//! GUI application state and SSH session management.

use sutty_core::config::SessionConfig;
use sutty_core::ssh::{SshEvent, SshSession};

/// Which state the connection is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnState {
    Disconnected,
    Connecting,
    Connected,
}

/// Saved connection parameters for auto-reconnect.
#[derive(Clone)]
struct SavedConnection {
    host: String,
    port: u16,
    username: String,
    password: Option<String>,
    key_file: Option<String>,
}

/// Represents a connected SSH session with its terminal emulator.
pub struct TermSession {
    pub parser: vt100::Parser,
    pub screen: vt100::Screen,
    pub cols: u16,
    pub write_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
}

impl TermSession {
    pub fn new(
        write_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
        rows: u16,
        cols: u16,
    ) -> Self {
        let parser = vt100::Parser::new(rows, cols, 0);
        let screen = parser.screen().clone();
        Self {
            parser,
            screen,
            cols,
            write_tx,
        }
    }

    pub fn process_data(&mut self, data: &[u8]) {
        self.parser.process(data);
        self.screen = self.parser.screen().clone();
    }

    pub fn cell(&self, row: u16, col: u16) -> Option<vt100::Cell> {
        self.screen.cell(row, col).cloned()
    }

    pub fn cursor_position(&self) -> (u16, u16) {
        self.screen.cursor_position()
    }

    pub fn screen_rows(&self) -> u16 {
        self.screen.size().0
    }

    /// Resize the terminal emulator to new dimensions.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.parser.set_size(rows, cols);
        self.screen = self.parser.screen().clone();
        self.cols = cols;
    }
}

/// Connection form state.
pub struct ConnectionForm {
    pub host: String,
    pub port: String,
    pub username: String,
    pub password: String,
    pub key_file: String,
    pub status: String,
    pub status_error: bool,
}

impl Default for ConnectionForm {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: String::from("22"),
            username: String::new(),
            password: String::new(),
            key_file: String::new(),
            status: String::new(),
            status_error: false,
        }
    }
}

impl ConnectionForm {
    pub fn port_u16(&self) -> u16 {
        self.port.parse().unwrap_or(22)
    }
}

/// The main application state.
pub struct RuttyApp {
    pub form: ConnectionForm,
    pub session: Option<TermSession>,
    pub read_rx: Option<tokio::sync::mpsc::UnboundedReceiver<SshEvent>>,
    pub pending_data: Vec<Vec<u8>>,
    pub should_close: bool,
    pub rt: tokio::runtime::Handle,

    // --- new fields ---
    /// Current connection state (drives button enable/disable).
    pub conn_state: ConnState,

    /// Blinking cursor state.
    pub cursor_visible: bool,
    cursor_timer: f64,

    /// Saved connection params for auto-reconnect.
    saved_conn: Option<SavedConnection>,

    /// Reconnect state.
    pub reconnect_count: u32,
    pub reconnect_max: u32,
    reconnect_delay: f64,

    /// "Save session?" prompt.
    pub show_save_prompt: bool,
    pub save_session_name: String,

    /// Sidebar visibility.
    pub show_sidebar: bool,

    /// Auto-reconnect toggle.
    pub auto_reconnect: bool,

    /// Auto-scroll terminal to bottom.
    pub auto_scroll: bool,
    /// Set to true when new data arrives — triggers one scroll-to-bottom.
    pub needs_scroll: bool,

    /// Saved sessions (loaded from disk).
    pub saved_sessions: Vec<SessionConfig>,
    /// Currently selected saved session name.
    pub selected_session: String,
    /// Snapshot of form fields when session was loaded.
    loaded_snapshot: Option<(String, String, String, String, String)>,
    /// Prompt: "Login data changed, save?"
    pub show_modify_prompt: bool,

    /// Window centered yet?
    centered: bool,
}

impl RuttyApp {
    pub fn new(rt: tokio::runtime::Handle) -> Self {
        let (mut sessions, _last) = sutty_core::config::SessionManager::load()
            .map(|mgr| (mgr.sessions().to_vec(), mgr.last_session.clone()))
            .unwrap_or_default();

        // Sort by last_used descending so the most recently used is first
        sessions.sort_by(|a, b| b.last_used.cmp(&a.last_used));

        let mut app = Self {
            form: ConnectionForm::default(),
            session: None,
            read_rx: None,
            pending_data: Vec::new(),
            should_close: false,
            rt,
            conn_state: ConnState::Disconnected,
            cursor_visible: true,
            cursor_timer: 0.0,
            saved_conn: None,
            reconnect_count: 0,
            reconnect_max: 5,
            reconnect_delay: 0.0,
            show_save_prompt: false,
            save_session_name: String::new(),
            show_sidebar: false,
            auto_reconnect: true,
            auto_scroll: true,
            needs_scroll: false,
            saved_sessions: sessions,
            selected_session: String::new(),
            loaded_snapshot: None,
            show_modify_prompt: false,
            centered: false,
        };

        // Auto-load most recently used session
        if let Some(cfg) = app.saved_sessions.first() {
            app.form.host = cfg.host.clone();
            app.form.port = cfg.port.to_string();
            app.form.username = cfg.username.clone();
            app.form.key_file = cfg.key_file.clone().unwrap_or_default();
            app.form.password = cfg.password().unwrap_or_default();
            app.selected_session = cfg.name.clone();
        }

        app
    }

    /// Whether the user can click Connect right now.
    pub fn can_connect(&self) -> bool {
        matches!(self.conn_state, ConnState::Disconnected)
    }

    /// Whether the user can click Disconnect right now.
    pub fn can_disconnect(&self) -> bool {
        matches!(
            self.conn_state,
            ConnState::Connected | ConnState::Connecting
        )
    }

    /// Apply a saved session to the form fields.
    pub fn apply_session(&mut self, name: &str) {
        if let Some(cfg) = self.saved_sessions.iter().find(|s| s.name == name) {
            self.form.host = cfg.host.clone();
            self.form.port = cfg.port.to_string();
            self.form.username = cfg.username.clone();
            self.form.key_file = cfg.key_file.clone().unwrap_or_default();
            self.form.password = cfg.password().unwrap_or_default();
            self.selected_session = name.to_string();
            // Snapshot the loaded values
            self.loaded_snapshot = Some((
                self.form.host.clone(),
                self.form.port.clone(),
                self.form.username.clone(),
                self.form.password.clone(),
                self.form.key_file.clone(),
            ));
        }
    }

    /// Check if form fields differ from the loaded session snapshot.
    pub fn is_form_modified(&self) -> bool {
        if let Some((ref h, ref p, ref u, ref pw, ref k)) = self.loaded_snapshot {
            self.form.host != *h
                || self.form.port != *p
                || self.form.username != *u
                || self.form.password != *pw
                || self.form.key_file != *k
        } else {
            false
        }
    }

    /// Delete a saved session by name.
    pub fn delete_session(&mut self, name: &str) {
        if let Ok(mut mgr) = sutty_core::config::SessionManager::load() {
            let _ = mgr.delete(name);
            self.saved_sessions = mgr.sessions().to_vec();
            if self.selected_session == name {
                self.selected_session.clear();
                self.form.host.clear();
                self.form.port = "22".into();
                self.form.username.clear();
                self.form.password.clear();
                self.form.key_file.clear();
            }
        }
    }

    /// Launch an SSH connection in a background task.
    pub fn connect(&mut self) {
        let host = self.form.host.clone();
        let port = self.form.port_u16();
        let username = self.form.username.clone();
        let password = if self.form.password.is_empty() {
            None
        } else {
            Some(self.form.password.clone())
        };
        let key_file = if self.form.key_file.is_empty() {
            None
        } else {
            Some(self.form.key_file.clone())
        };

        if host.is_empty() || username.is_empty() {
            self.form.status = "Host and username are required".into();
            self.form.status_error = true;
            return;
        }

        // Save params for auto-reconnect
        self.saved_conn = Some(SavedConnection {
            host: host.clone(),
            port,
            username: username.clone(),
            password: password.clone(),
            key_file: key_file.clone(),
        });
        self.reconnect_count = 0;

        self.form.status = format!("Connecting to {}@{}:{}...", username, host, port);
        self.form.status_error = false;
        self.conn_state = ConnState::Connecting;

        let (write_tx, mut write_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let (read_tx, read_rx) = tokio::sync::mpsc::unbounded_channel::<SshEvent>();

        self.read_rx = Some(read_rx);

        let term_rows: u16 = 30;
        let term_cols: u16 = 100;
        self.session = Some(TermSession::new(write_tx, term_rows, term_cols));

        let rt = self.rt.clone();
        rt.spawn(async move {
            let mut ssh = match SshSession::connect(&host, port, &username, password, key_file.as_deref()).await {
                Ok(s) => s,
                Err(e) => {
                    let _ = read_tx.send(SshEvent::Data(
                        format!("\r\n\x1b[31mConnection failed: {}\x1b[0m\r\n", e).into_bytes(),
                    ));
                    let _ = read_tx.send(SshEvent::Eof);
                    return;
                }
            };

            if let Err(e) = ssh.request_pty(term_cols as u32, term_rows as u32, "xterm-256color").await {
                let _ = read_tx.send(SshEvent::Data(
                    format!("\r\n\x1b[31mPTY failed: {}\x1b[0m\r\n", e).into_bytes(),
                ));
                let _ = read_tx.send(SshEvent::Eof);
                return;
            }

            // I/O loop
            loop {
                tokio::select! {
                    maybe_data = write_rx.recv() => {
                        match maybe_data {
                            Some(data) => { if ssh.send_data(&data).await.is_err() { break; } }
                            None => break,
                        }
                    }
                    maybe_event = ssh.receive() => {
                        match maybe_event {
                            Some(event) => {
                                let is_terminal = matches!(event, SshEvent::Eof | SshEvent::ExitStatus(_));
                                let _ = read_tx.send(event);
                                if is_terminal { break; }
                            }
                            None => break,
                        }
                    }
                }
            }

            let _ = ssh.close().await;
        });
    }

    /// Disconnect and stop reconnect attempts.
    pub fn disconnect(&mut self) {
        self.session = None;
        self.read_rx = None;
        self.pending_data.clear();
        self.saved_conn = None;
        self.conn_state = ConnState::Disconnected;
        self.reconnect_count = 0;
        self.show_sidebar = false;
        self.form.status = "Disconnected.".into();
        self.form.status_error = false;
    }

    /// Send keystrokes to the SSH session.
    pub fn send_keys(&self, data: Vec<u8>) {
        if let Some(ref session) = self.session {
            let _ = session.write_tx.send(data);
        }
    }

    /// Poll for incoming SSH data (call each frame from egui).
    pub fn poll_ssh_data(&mut self) {
        if let Some(ref mut rx) = self.read_rx {
            while let Ok(event) = rx.try_recv() {
                match event {
                    SshEvent::Data(data) => {
                        // First data means we're connected
                        if self.conn_state == ConnState::Connecting {
                            self.conn_state = ConnState::Connected;
                            self.form.status = "Connected.".into();
                            self.form.status_error = false;
                            if self.reconnect_count == 0 && !self.show_save_prompt {
                                let already_saved = sutty_core::config::SessionManager::load()
                                    .ok()
                                    .map(|mgr| {
                                        mgr.sessions().iter().any(|s| {
                                            s.host == self.form.host && s.username == self.form.username
                                        })
                                    })
                                    .unwrap_or(false);
                                if !already_saved {
                                    self.show_save_prompt = true;
                                    self.save_session_name =
                                        format!("{}@{}", self.form.username, self.form.host);
                                }
                            }
                        }
                        self.pending_data.push(data);
                        self.needs_scroll = true;
                    }
                    SshEvent::Eof | SshEvent::ExitStatus(_) => {
                        if self.conn_state == ConnState::Connecting {
                            // Failed before connecting — show error
                            self.form.status = format!(
                                "Connection failed{}.",
                                if self.reconnect_count > 0 {
                                    format!(" (attempt {})", self.reconnect_count)
                                } else {
                                    String::new()
                                }
                            );
                            self.form.status_error = true;
                        } else {
                            self.form.status = "Connection closed.".into();
                            self.form.status_error = false;
                        }
                        self.conn_state = ConnState::Disconnected;
                        self.reconnect_delay = 5.0;
                    }
                }
            }
        }
    }

    /// Process all pending SSH data through the terminal parser.
    pub fn process_pending(&mut self) {
        if let Some(ref mut session) = self.session {
            for data in self.pending_data.drain(..) {
                session.process_data(&data);
            }
        }
    }

    /// Tick timers: cursor blink and auto-reconnect.
    pub fn tick(&mut self, dt: f64) {
        // Cursor blink (500ms on, 500ms off)
        self.cursor_timer += dt;
        if self.cursor_timer >= 0.5 {
            self.cursor_timer -= 0.5;
            self.cursor_visible = !self.cursor_visible;
        }

        // Auto-reconnect (only if enabled)
        if self.auto_reconnect
            && self.conn_state == ConnState::Disconnected
            && self.saved_conn.is_some()
            && self.reconnect_count < self.reconnect_max
        {
            self.reconnect_delay -= dt;
            if self.reconnect_delay <= 0.0 {
                self.reconnect_count += 1;
                let sc = self.saved_conn.as_ref().unwrap();
                self.form.host = sc.host.clone();
                self.form.port = sc.port.to_string();
                self.form.username = sc.username.clone();
                self.form.password = sc.password.clone().unwrap_or_default();
                self.form.key_file = sc.key_file.clone().unwrap_or_default();
                self.form.status = format!(
                    "Reconnecting (attempt {}/{})...",
                    self.reconnect_count, self.reconnect_max
                );
                self.form.status_error = false;
                self.connect();
            }
        }
    }

    /// Save the current form as a named session.
    pub fn save_session(&mut self) {
        if self.save_session_name.is_empty() {
            return;
        }
        let mut mgr = match sutty_core::config::SessionManager::load() {
            Ok(m) => m,
            Err(e) => {
                self.form.status = format!("Failed to load sessions: {}", e);
                self.form.status_error = true;
                return;
            }
        };
        let mut cfg = sutty_core::config::SessionConfig {
            name: self.save_session_name.clone(),
            host: self.form.host.clone(),
            port: self.form.port_u16(),
            username: self.form.username.clone(),
            key_file: if self.form.key_file.is_empty() {
                None
            } else {
                Some(self.form.key_file.clone())
            },
            encrypted_password: None,
            last_used: 0,
        };
        // Store password if provided
        if !self.form.password.is_empty() {
            cfg.set_password(&self.form.password);
        }
        match mgr.upsert(cfg) {
            Ok(()) => {
                self.form.status = format!("Session '{}' saved.", self.save_session_name);
                self.form.status_error = false;
                // Refresh saved sessions list
                self.saved_sessions = mgr.sessions().to_vec();
                self.selected_session = self.save_session_name.clone();
            }
            Err(e) => {
                self.form.status = format!("Save failed: {}", e);
                self.form.status_error = true;
            }
        }
        self.show_save_prompt = false;
    }

    /// Calculate reconnect delay text for status bar.
    pub fn reconnect_status(&self) -> Option<String> {
        if self.conn_state == ConnState::Disconnected
            && self.saved_conn.is_some()
            && self.reconnect_count < self.reconnect_max
        {
            Some(format!(
                "Reconnecting in {:.0}s (attempt {}/{})…",
                self.reconnect_delay.max(0.0).ceil(),
                self.reconnect_count + 1,
                self.reconnect_max
            ))
        } else {
            None
        }
    }
}

impl eframe::App for RuttyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Center the window on first frame + apply dark theme
        if !self.centered {
            self.centered = true;
            apply_dark_theme(ctx);
            if let Some(monitor) = ctx.input(|i| i.viewport().monitor_size) {
                let win_size = egui::Vec2::new(860.0, 520.0);
                let pos = egui::Pos2::new(
                    ((monitor.x - win_size.x) / 2.0).max(0.0),
                    ((monitor.y - win_size.y) / 2.0).max(0.0),
                );
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
            }
        }

        let dt = ctx.input(|i| i.unstable_dt) as f64;
        self.tick(dt);

        self.poll_ssh_data();
        self.process_pending();

        // Repaint continuously while connected (for cursor blink + live terminal)
        if self.conn_state != ConnState::Disconnected {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        } else if self.saved_conn.is_some() && self.reconnect_count < self.reconnect_max {
            // Reconnect countdown — repaint to update the timer display
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }

        crate::ui::render(self, ctx);
    }
}

/// Apply a custom dark theme to the entire UI.
fn apply_dark_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();

    // Dark background colors
    style.visuals.window_fill = egui::Color32::from_rgb(18, 18, 24);
    style.visuals.panel_fill = egui::Color32::from_rgb(22, 22, 30);
    style.visuals.faint_bg_color = egui::Color32::from_rgb(28, 28, 38);
    style.visuals.extreme_bg_color = egui::Color32::from_rgb(12, 12, 18);
    style.visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(32, 32, 42);
    style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(38, 38, 50);
    style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(50, 50, 68);
    style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(60, 60, 82);

    // Text colors
    style.visuals.widgets.noninteractive.fg_stroke.color = egui::Color32::from_rgb(200, 200, 210);
    style.visuals.widgets.inactive.fg_stroke.color = egui::Color32::from_rgb(180, 180, 195);
    style.visuals.widgets.hovered.fg_stroke.color = egui::Color32::WHITE;
    style.visuals.widgets.active.fg_stroke.color = egui::Color32::WHITE;

    // Accent color (cyan/teal)
    style.visuals.selection.bg_fill = egui::Color32::from_rgb(0, 150, 180);
    style.visuals.hyperlink_color = egui::Color32::from_rgb(0, 200, 220);

    // Spacing
    style.spacing.item_spacing = egui::Vec2::new(8.0, 6.0);
    style.spacing.button_padding = egui::Vec2::new(14.0, 6.0);

    ctx.set_style(style);
}
