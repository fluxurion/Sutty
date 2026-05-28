//! UI rendering for Sutty GUI using egui.
//!
//! Layout: menu bar | sidebar (toggleable) | terminal view | status bar.

use crate::app::{ConnState, RuttyApp};
use egui::{Align, Color32, Key, Modifiers, ScrollArea};

pub fn render(app: &mut RuttyApp, ctx: &egui::Context) {
    let time = ctx.input(|i| i.time) as f32;

    // ── Menu bar + toolbar ────────────────────────────────────
    egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("Session", |ui| {
                if ui
                    .add_enabled(app.can_connect(), egui::Button::new("New Connection..."))
                    .clicked()
                {}
                if ui
                    .add_enabled(app.can_disconnect(), egui::Button::new("Disconnect"))
                    .clicked()
                {
                    app.disconnect();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    app.should_close = true;
                }
            });

            // ── Toolbar buttons (always visible) ──────────────
            if app.can_disconnect() {
                if ui.button("⏻ Disconnect").clicked() {
                    app.disconnect();
                }
                if ui
                    .button(if app.show_sidebar {
                        "◀ Hide"
                    } else {
                        "▶ Show"
                    })
                    .clicked()
                {
                    app.show_sidebar = !app.show_sidebar;
                }
                ui.checkbox(&mut app.auto_reconnect, "Auto-reconnect");
                ui.checkbox(&mut app.auto_scroll, "Auto-scroll");
            }

            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                render_status_dot(ui, app.conn_state, time);
                let (label, color) = match app.conn_state {
                    ConnState::Connected => ("Connected", Color32::from_rgb(100, 220, 140)),
                    ConnState::Connecting => ("Connecting…", Color32::from_rgb(220, 200, 80)),
                    ConnState::Disconnected => ("Disconnected", Color32::from_rgb(140, 140, 150)),
                };
                ui.label(egui::RichText::new(label).color(color));
            });
        });
    });

    // ── Main area ─────────────────────────────────────────────
    egui::CentralPanel::default().show(ctx, |ui| {
        if app.conn_state != ConnState::Disconnected {
            // Sidebar (conditionally rendered)
            if app.show_sidebar {
                egui::SidePanel::left("connection_panel")
                    .resizable(true)
                    .default_width(200.0)
                    .show_inside(ui, |ui| {
                        egui::Frame::new()
                            .fill(Color32::from_rgb(26, 26, 36))
                            .corner_radius(6)
                            .inner_margin(egui::Margin::symmetric(8, 6))
                            .show(ui, |ui| {
                                render_connection_form(app, ui, true);
                            });
                    });
            }

            // Terminal — fill remaining space exactly
            egui::Frame::new()
                .fill(Color32::from_rgb(12, 12, 18))
                .corner_radius(4)
                .inner_margin(egui::Margin::same(0))
                .outer_margin(egui::Margin::same(0))
                .show(ui, |ui| {
                    render_terminal(app, ui);
                });
        } else {
            egui::CentralPanel::default().show_inside(ui, |ui| {
                render_connection_form(app, ui, false);
            });
        }
    });

    // ── Save session prompt ───────────────────────────────────
    if app.show_save_prompt {
        egui::Window::new("Save Session?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("Connection successful! Save these settings for later?");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    ui.text_edit_singleline(&mut app.save_session_name);
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        app.save_session();
                    }
                    if ui.button("Skip").clicked() {
                        app.show_save_prompt = false;
                    }
                });
            });
    }

    // ── Modify prompt ────────────────────────────────────────
    if app.show_modify_prompt {
        egui::Window::new("Login Data Changed")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(format!(
                    "Login data for '{}' has been modified.",
                    app.selected_session
                ));
                ui.label("Would you like to save the changes?");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Save && Connect").clicked() {
                        app.save_session_name = app.selected_session.clone();
                        app.save_session();
                        app.show_modify_prompt = false;
                        app.connect();
                    }
                    if ui.button("Discard && Connect").clicked() {
                        app.show_modify_prompt = false;
                        app.connect();
                    }
                    if ui.button("Cancel").clicked() {
                        app.show_modify_prompt = false;
                    }
                });
            });
    }

    // ── Status bar ────────────────────────────────────────────
    egui::TopBottomPanel::bottom("status_bar")
        .min_height(20.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(recon) = app.reconnect_status() {
                    ui.colored_label(Color32::YELLOW, &recon);
                } else if !app.form.status.is_empty() {
                    if app.form.status_error {
                        ui.colored_label(Color32::RED, &app.form.status);
                    } else {
                        ui.label(&app.form.status);
                    }
                } else {
                    ui.label("Ready");
                }
            });
        });

    handle_keyboard_input(app, ctx);
    if app.should_close {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

/// Connection form.
fn render_connection_form(app: &mut RuttyApp, ui: &mut egui::Ui, compact: bool) {
    if compact {
        ui.heading("Connection");
        ui.separator();
    } else {
        ui.vertical_centered(|ui| {
            ui.heading("Sutty — SSH Client");
            ui.add_space(10.0);
        });
    }
    let available = ui.available_width();
    let enabled = app.can_connect();

    // ── Saved sessions dropdown ──────────────────────────────
    if !app.saved_sessions.is_empty() {
        ui.horizontal(|ui| {
            ui.label("Saved:");
            let combo_width = (available - 95.0).max(60.0);
            egui::ComboBox::from_label("")
                .width(combo_width)
                .selected_text(if app.selected_session.is_empty() {
                    "(select to load)"
                } else {
                    &app.selected_session
                })
                .show_ui(ui, |ui| {
                    let sessions = app.saved_sessions.clone();
                    for s in &sessions {
                        let label = format!("{}  ({}@{})", s.name, s.username, s.host);
                        if ui.selectable_label(false, &label).clicked() {
                            app.apply_session(&s.name);
                        }
                    }
                });
            // Delete button — always visible, disabled when nothing selected
            ui.add_enabled_ui(!app.selected_session.is_empty(), |ui| {
                if ui
                    .button("🗑")
                    .on_hover_text("Delete saved session")
                    .clicked()
                {
                    let name = app.selected_session.clone();
                    app.delete_session(&name);
                }
            });
        });
        ui.add_space(6.0);
    }

    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal(|ui| {
            ui.label("Host:");
            ui.add_sized(
                [available - 40.0, 20.0],
                egui::TextEdit::singleline(&mut app.form.host).hint_text("hostname or IP"),
            );
        });
    });
    ui.add_space(4.0);
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal(|ui| {
            ui.label("Port:");
            ui.add_sized(
                [80.0, 20.0],
                egui::TextEdit::singleline(&mut app.form.port).hint_text("22"),
            );
        });
    });
    ui.add_space(4.0);
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal(|ui| {
            ui.label("User:");
            ui.add_sized(
                [available - 40.0, 20.0],
                egui::TextEdit::singleline(&mut app.form.username).hint_text("username"),
            );
        });
    });
    ui.add_space(4.0);
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal(|ui| {
            ui.label("Pass:");
            ui.add_sized(
                [available - 40.0, 20.0],
                egui::TextEdit::singleline(&mut app.form.password)
                    .password(true)
                    .hint_text("password (or use key)"),
            );
        });
    });
    ui.add_space(4.0);
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal(|ui| {
            ui.label("Key:");
            ui.add_sized(
                [available - 40.0, 20.0],
                egui::TextEdit::singleline(&mut app.form.key_file)
                    .hint_text("path to private key (optional)"),
            );
        });
    });
    ui.add_space(8.0);

    ui.horizontal(|ui| {
        let btn_text = match app.conn_state {
            ConnState::Connecting => "Connecting…",
            ConnState::Connected => "Connected",
            ConnState::Disconnected => "Connect",
        };
        let btn = egui::Button::new(egui::RichText::new(btn_text).color(Color32::WHITE).strong())
            .fill(Color32::from_rgb(0, 150, 180))
            .min_size(egui::Vec2::new(100.0, 28.0));
        if ui.add_enabled(enabled, btn).clicked() {
            try_connect(app);
        }
        if ui
            .add_enabled(app.can_disconnect(), egui::Button::new("Disconnect"))
            .clicked()
        {
            app.disconnect();
        }
    });

    ui.add_space(8.0);
    ui.label("Keys are sent to the remote host while connected.");

    // Pressing Enter on any field triggers connect
    if enabled
        && ui.ctx().input(|i| i.key_pressed(egui::Key::Enter))
        && !app.form.host.is_empty()
        && !app.form.username.is_empty()
    {
        try_connect(app);
    }
}

/// Check for modified data before connecting.
fn try_connect(app: &mut RuttyApp) {
    if !app.selected_session.is_empty() && app.is_form_modified() {
        app.show_modify_prompt = true;
    } else {
        app.connect();
    }
}

/// Render the terminal view with auto-scroll and cursor padding.
fn render_terminal(app: &mut RuttyApp, ui: &mut egui::Ui) {
    let session = match app.session.as_ref() {
        Some(s) => s,
        None => {
            ui.centered_and_justified(|ui| {
                ui.label("Not connected.");
            });
            return;
        }
    };

    let rows = session.screen_rows();
    let cols = session.cols;
    let font_id = egui::FontId::monospace(14.0);
    let row_height = ui.fonts(|f| f.row_height(&font_id));
    let (cur_row, cur_col) = session.cursor_position();

    // Measure a single character width for cell-based painting
    let char_width = ui.fonts(|f| f.glyph_width(&font_id, 'W'));

    // Calculate desired scroll for cursor visibility
    let viewport_h = ui.available_height();
    let visible_rows = (viewport_h / row_height).floor().max(5.0) as u16;

    // Auto-scroll: always scroll to cursor when enabled (not just on new data).
    // This handles cases like htop exiting where the terminal switches back from
    // the alternate screen without producing new network data.
    let do_scroll = app.auto_scroll;

    ScrollArea::both()
        .auto_shrink([false, true])
        .show(ui, |ui| {
            if do_scroll {
                let scroll_y = (cur_row as f32 - visible_rows as f32 * 0.35).max(0.0) * row_height;
                ui.scroll_to_rect(
                    egui::Rect::from_min_size(
                        egui::Pos2::new(0.0, scroll_y),
                        egui::Vec2::new(1.0, 1.0),
                    ),
                    Some(Align::Min),
                );
            }

            let painter = ui.painter().clone();

            for row in 0..rows {
                ui.horizontal(|ui| {
                    ui.set_height(row_height);
                    ui.spacing_mut().item_spacing.x = 0.0;

                    // First pass: build text runs and collect background colors
                    struct Run {
                        text: String,
                        fg: Color32,
                        bg: Color32,
                    }

                    let mut runs: Vec<Run> = Vec::new();
                    let mut cur_run = Run {
                        text: String::new(),
                        fg: Color32::from_rgb(200, 210, 200),
                        bg: Color32::TRANSPARENT,
                    };

                    for col in 0..cols {
                        let (ch, fg, bg) = if let Some(ref cell) = session.cell(row, col) {
                            let contents = cell.contents();
                            let ch = if contents.is_empty() {
                                ' '
                            } else {
                                contents.chars().next().unwrap_or(' ')
                            };
                            (
                                ch,
                                vt100_fg_to_egui(cell.fgcolor()),
                                vt100_bg_to_egui(cell.bgcolor()),
                            )
                        } else {
                            (' ', Color32::from_rgb(200, 210, 200), Color32::TRANSPARENT)
                        };

                        let display_ch = if app.cursor_visible
                            && app.conn_state == ConnState::Connected
                            && row == cur_row
                            && col == cur_col
                        {
                            '▌'
                        } else {
                            ch
                        };

                        if fg != cur_run.fg || bg != cur_run.bg {
                            if !cur_run.text.is_empty() {
                                runs.push(Run {
                                    text: std::mem::take(&mut cur_run.text),
                                    fg: cur_run.fg,
                                    bg: cur_run.bg,
                                });
                            }
                            cur_run.fg = fg;
                            cur_run.bg = bg;
                        }
                        cur_run.text.push(display_ch);
                    }

                    let trimmed = cur_run.text.trim_end().to_string();
                    if !trimmed.is_empty() {
                        runs.push(Run {
                            text: trimmed,
                            fg: cur_run.fg,
                            bg: cur_run.bg,
                        });
                    }

                    // Second pass: paint background rects, then text
                    let base_pos = ui.next_widget_position();
                    let mut x_offset = 0.0;

                    // Paint backgrounds
                    for run in &runs {
                        if run.bg != Color32::TRANSPARENT {
                            let run_w = run.text.len() as f32 * char_width;
                            let rect = egui::Rect::from_min_size(
                                egui::Pos2::new(base_pos.x + x_offset, base_pos.y),
                                egui::Vec2::new(run_w, row_height),
                            );
                            painter.rect_filled(rect, 0.0, run.bg);
                        }
                        x_offset += run.text.len() as f32 * char_width;
                    }

                    // Draw text (on top of backgrounds)
                    for run in &runs {
                        ui.label(
                            egui::RichText::new(&run.text)
                                .font(font_id.clone())
                                .color(run.fg)
                                .background_color(run.bg),
                        );
                    }
                });
            }

            // Padding rows so you can scroll past the last line
            let pad_rows = 1;
            for _ in 0..pad_rows {
                ui.horizontal(|ui| {
                    ui.set_height(row_height);
                    ui.label("");
                });
            }
        });
}

/// Handle keyboard input.
fn handle_keyboard_input(app: &RuttyApp, ctx: &egui::Context) {
    // Only forward keys when fully connected (not during connecting phase)
    if app.conn_state == ConnState::Connected {
        ctx.input_mut(|i| {
            while let Some(event) = i.events.pop() {
                match event {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        let bytes = key_to_ssh_bytes(key, modifiers);
                        if !bytes.is_empty() {
                            app.send_keys(bytes);
                        }
                    }
                    egui::Event::Text(text) => {
                        if !text.is_empty() && !text.starts_with('\u{1b}') {
                            app.send_keys(text.as_bytes().to_vec());
                        }
                    }
                    _ => {}
                }
            }
        });
    }
}

fn key_to_ssh_bytes(key: Key, mods: Modifiers) -> Vec<u8> {
    if mods.ctrl {
        match key {
            Key::A => return vec![1],
            Key::B => return vec![2],
            Key::C => return vec![3],
            Key::D => return vec![4],
            Key::E => return vec![5],
            Key::F => return vec![6],
            Key::G => return vec![7],
            Key::H => return vec![8],
            Key::I => return vec![9],
            Key::J => return vec![10],
            Key::K => return vec![11],
            Key::L => return vec![12],
            Key::M => return vec![13],
            Key::N => return vec![14],
            Key::O => return vec![15],
            Key::P => return vec![16],
            Key::Q => return vec![17],
            Key::R => return vec![18],
            Key::S => return vec![19],
            Key::T => return vec![20],
            Key::U => return vec![21],
            Key::V => return vec![22],
            Key::W => return vec![23],
            Key::X => return vec![24],
            Key::Y => return vec![25],
            Key::Z => return vec![26],
            Key::OpenBracket => return vec![27],
            Key::Backslash => return vec![28],
            Key::CloseBracket => return vec![29],
            Key::Num6 => return vec![30],
            Key::Minus => return vec![31],
            _ => {}
        }
    }
    match key {
        Key::Enter => vec![b'\r'],
        Key::Tab => vec![b'\t'],
        Key::Backspace => vec![0x7f],
        Key::Escape => vec![0x1b],
        Key::ArrowUp => vec![0x1b, b'[', b'A'],
        Key::ArrowDown => vec![0x1b, b'[', b'B'],
        Key::ArrowRight => vec![0x1b, b'[', b'C'],
        Key::ArrowLeft => vec![0x1b, b'[', b'D'],
        Key::Home => vec![0x1b, b'[', b'H'],
        Key::End => vec![0x1b, b'[', b'F'],
        Key::PageUp => vec![0x1b, b'[', b'5', b'~'],
        Key::PageDown => vec![0x1b, b'[', b'6', b'~'],
        Key::Delete => vec![0x1b, b'[', b'3', b'~'],
        Key::Insert => vec![0x1b, b'[', b'2', b'~'],
        Key::F1 => vec![0x1b, b'O', b'P'],
        Key::F2 => vec![0x1b, b'O', b'Q'],
        Key::F3 => vec![0x1b, b'O', b'R'],
        Key::F4 => vec![0x1b, b'O', b'S'],
        Key::F5 => vec![0x1b, b'[', b'1', b'5', b'~'],
        Key::F6 => vec![0x1b, b'[', b'1', b'7', b'~'],
        Key::F7 => vec![0x1b, b'[', b'1', b'8', b'~'],
        Key::F8 => vec![0x1b, b'[', b'1', b'9', b'~'],
        Key::F9 => vec![0x1b, b'[', b'2', b'0', b'~'],
        Key::F10 => vec![0x1b, b'[', b'2', b'1', b'~'],
        Key::F11 => vec![0x1b, b'[', b'2', b'3', b'~'],
        Key::F12 => vec![0x1b, b'[', b'2', b'4', b'~'],
        _ => vec![],
    }
}

fn vt100_fg_to_egui(c: vt100::Color) -> Color32 {
    match c {
        vt100::Color::Default => Color32::from_rgb(200, 210, 200),
        vt100::Color::Idx(i) => match i {
            0 => Color32::BLACK,
            1 => Color32::from_rgb(205, 0, 0),
            2 => Color32::from_rgb(0, 205, 0),
            3 => Color32::from_rgb(205, 205, 0),
            4 => Color32::from_rgb(0, 0, 205),
            5 => Color32::from_rgb(205, 0, 205),
            6 => Color32::from_rgb(0, 205, 205),
            7 => Color32::from_rgb(229, 229, 229),
            8 => Color32::from_rgb(127, 127, 127),
            9 => Color32::from_rgb(255, 0, 0),
            10 => Color32::from_rgb(0, 255, 0),
            11 => Color32::from_rgb(255, 255, 0),
            12 => Color32::from_rgb(0, 0, 255),
            13 => Color32::from_rgb(255, 0, 255),
            14 => Color32::from_rgb(0, 255, 255),
            15 => Color32::from_rgb(255, 255, 255),
            n if n < 232 => {
                let n = n - 16;
                Color32::from_rgb((n / 36) * 51, ((n / 6) % 6) * 51, (n % 6) * 51)
            }
            n => {
                let g = ((n - 232) * 10 + 8) as u8;
                Color32::from_rgb(g, g, g)
            }
        },
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
    }
}

fn vt100_bg_to_egui(c: vt100::Color) -> Color32 {
    match c {
        vt100::Color::Default => Color32::TRANSPARENT,
        vt100::Color::Idx(i) => match i {
            0 => Color32::from_rgb(0, 0, 0),
            1 => Color32::from_rgb(205, 50, 50),
            2 => Color32::from_rgb(50, 205, 50),
            3 => Color32::from_rgb(205, 205, 50),
            4 => Color32::from_rgb(50, 50, 205),
            5 => Color32::from_rgb(205, 50, 205),
            6 => Color32::from_rgb(50, 205, 205),
            7 => Color32::from_rgb(229, 229, 229),
            8 => Color32::from_rgb(127, 127, 127),
            9 => Color32::from_rgb(255, 80, 80),
            10 => Color32::from_rgb(80, 255, 80),
            11 => Color32::from_rgb(255, 255, 80),
            12 => Color32::from_rgb(80, 80, 255),
            13 => Color32::from_rgb(255, 80, 255),
            14 => Color32::from_rgb(80, 255, 255),
            15 => Color32::from_rgb(255, 255, 255),
            n if n < 232 => {
                let n = n - 16;
                Color32::from_rgb((n / 36) * 51, ((n / 6) % 6) * 51, (n % 6) * 51)
            }
            n => {
                let g = ((n - 232) * 10 + 8) as u8;
                Color32::from_rgb(g, g, g)
            }
        },
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
    }
}

fn render_status_dot(ui: &mut egui::Ui, state: ConnState, time: f32) {
    let (base_color, is_active) = match state {
        ConnState::Connected => (Color32::from_rgb(60, 220, 120), true),
        ConnState::Connecting => (Color32::from_rgb(220, 200, 60), true),
        ConnState::Disconnected => (Color32::from_rgb(100, 100, 110), false),
    };
    let alpha = if is_active {
        0.6 + 0.4 * (time * 3.0).sin().abs()
    } else {
        0.4
    };
    let color = Color32::from_rgba_premultiplied(
        (base_color.r() as f32 * alpha) as u8,
        (base_color.g() as f32 * alpha) as u8,
        (base_color.b() as f32 * alpha) as u8,
        (255.0 * alpha) as u8,
    );
    let size = 10.0;
    let (rect, _) = ui.allocate_exact_size(egui::Vec2::new(size + 8.0, size), egui::Sense::hover());
    let dot_center = rect.center();
    ui.painter().circle_filled(dot_center, size / 2.0, color);
    if is_active {
        let glow_alpha = alpha * 0.3;
        let glow_color = Color32::from_rgba_premultiplied(
            (base_color.r() as f32 * glow_alpha) as u8,
            (base_color.g() as f32 * glow_alpha) as u8,
            (base_color.b() as f32 * glow_alpha) as u8,
            (255.0 * glow_alpha) as u8,
        );
        ui.painter()
            .circle_filled(dot_center, size / 2.0 + 3.0, glow_color);
    }
}
