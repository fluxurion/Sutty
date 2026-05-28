//! Application state for the TUI connection dialog.

use sutty_core::config::SessionManager;

/// Represents which screen the TUI is currently showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    ConnectionDialog,
    SessionList,
}

/// Fields editable in the connection dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Host,
    Port,
    Username,
    Password,
    KeyFile,
    SessionName,
}

/// Outcome of the TUI dialog.
#[derive(Debug, Clone)]
pub enum DialogResult {
    /// User wants to connect with these parameters.
    Connect {
        host: String,
        port: u16,
        username: String,
        password: String,
        key_file: Option<String>,
    },
    /// User chose to quit without connecting.
    Quit,
}

/// Main TUI application state for Sutty's connection GUI.
pub struct App {
    pub screen: Screen,
    pub field: Field,
    pub host: String,
    pub port_str: String,
    pub username: String,
    pub password: String,
    pub key_file: String,
    pub session_name: String,
    pub sessions: SessionManager,
    pub session_list_index: usize,
    pub status_msg: String,
    pub status_is_error: bool,
    pub result: Option<DialogResult>,
}

impl App {
    pub fn new(sessions: SessionManager) -> Self {
        Self {
            screen: Screen::ConnectionDialog,
            field: Field::Host,
            host: String::new(),
            port_str: String::from("22"),
            username: String::new(),
            password: String::new(),
            key_file: String::new(),
            session_name: String::new(),
            sessions,
            session_list_index: 0,
            status_msg: String::new(),
            status_is_error: false,
            result: None,
        }
    }

    pub fn port(&self) -> u16 {
        self.port_str.parse().unwrap_or(22)
    }

    pub fn set_status(&mut self, msg: &str, is_error: bool) {
        self.status_msg = msg.to_string();
        self.status_is_error = is_error;
    }

    pub fn clear_status(&mut self) {
        self.status_msg.clear();
    }

    /// Move focus to the next editable field.
    pub fn next_field(&mut self) {
        use Field::*;
        self.field = match self.field {
            Host => Port,
            Port => Username,
            Username => Password,
            Password => KeyFile,
            KeyFile => SessionName,
            SessionName => Host,
        };
    }

    /// Move focus to the previous editable field.
    pub fn prev_field(&mut self) {
        use Field::*;
        self.field = match self.field {
            Host => SessionName,
            SessionName => KeyFile,
            KeyFile => Password,
            Password => Username,
            Username => Port,
            Port => Host,
        };
    }

    /// Populate fields from a saved session config.
    pub fn load_session(&mut self, index: usize) {
        if let Some(s) = self.sessions.sessions().get(index) {
            self.host = s.host.clone();
            self.port_str = s.port.to_string();
            self.username = s.username.clone();
            self.key_file = s.key_file.clone().unwrap_or_default();
            self.session_name = s.name.clone();
            self.screen = Screen::ConnectionDialog;
            self.set_status(&format!("Loaded session '{}'", s.name), false);
        }
    }

    /// Save the current fields as a named session.
    pub fn save_current_session(&mut self) {
        if self.session_name.is_empty() {
            self.set_status("Session name is required to save", true);
            return;
        }
        let cfg = sutty_core::config::SessionConfig {
            name: self.session_name.clone(),
            host: self.host.clone(),
            port: self.port(),
            username: self.username.clone(),
            key_file: if self.key_file.is_empty() {
                None
            } else {
                Some(self.key_file.clone())
            },
            encrypted_password: None,
            last_used: 0,
        };
        match self.sessions.upsert(cfg) {
            Ok(()) => self.set_status(&format!("Session '{}' saved", self.session_name), false),
            Err(e) => self.set_status(&format!("Save failed: {}", e), true),
        }
    }

    /// Delete the currently selected session from the list.
    pub fn delete_selected_session(&mut self) {
        if let Some(s) = self.sessions.sessions().get(self.session_list_index) {
            let name = s.name.clone();
            if let Err(e) = self.sessions.delete(&name) {
                self.set_status(&format!("Delete failed: {}", e), true);
            } else {
                if self.session_list_index >= self.sessions.sessions().len() {
                    self.session_list_index = self.session_list_index.saturating_sub(1);
                }
                self.set_status(&format!("Deleted session '{}'", name), false);
            }
        }
    }
}
