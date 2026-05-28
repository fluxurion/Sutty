//! Renders the PuTTY-like connection dialog and saved-sessions list using ratatui.

use crate::tui::app::{App, DialogResult, Field, Screen};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

/// Handle a key event in the TUI, returning `true` if the dialog wants to exit.
pub fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    if key.kind == KeyEventKind::Release {
        return false;
    }

    match app.screen {
        Screen::ConnectionDialog => handle_connection_key(app, key),
        Screen::SessionList => handle_session_list_key(app, key),
    }
}

fn handle_connection_key(app: &mut App, key: KeyEvent) -> bool {
    app.clear_status();

    match key.code {
        KeyCode::Esc => {
            app.result = Some(DialogResult::Quit);
            return true;
        }
        KeyCode::Tab => app.next_field(),
        KeyCode::BackTab => app.prev_field(),
        KeyCode::Enter => {
            if app.host.is_empty() {
                app.set_status("Hostname is required", true);
            } else if app.username.is_empty() {
                app.set_status("Username is required", true);
            } else {
                app.result = Some(DialogResult::Connect {
                    host: app.host.clone(),
                    port: app.port(),
                    username: app.username.clone(),
                    password: app.password.clone(),
                    key_file: if app.key_file.is_empty() {
                        None
                    } else {
                        Some(app.key_file.clone())
                    },
                });
                return true;
            }
        }
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.save_current_session();
        }
        KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.screen = Screen::SessionList;
        }
        KeyCode::Backspace => match app.field {
            Field::Host => {
                app.host.pop();
            }
            Field::Port => {
                app.port_str.pop();
            }
            Field::Username => {
                app.username.pop();
            }
            Field::Password => {
                app.password.pop();
            }
            Field::KeyFile => {
                app.key_file.pop();
            }
            Field::SessionName => {
                app.session_name.pop();
            }
        },
        KeyCode::Char(c) => {
            let target: &mut String = match app.field {
                Field::Host => &mut app.host,
                Field::Port => &mut app.port_str,
                Field::Username => &mut app.username,
                Field::Password => &mut app.password,
                Field::KeyFile => &mut app.key_file,
                Field::SessionName => &mut app.session_name,
            };
            // Port field: only allow digits
            if matches!(app.field, Field::Port) && !c.is_ascii_digit() {
                return false;
            }
            target.push(c);
        }
        _ => {}
    }
    false
}

fn handle_session_list_key(app: &mut App, key: KeyEvent) -> bool {
    app.clear_status();
    let sessions = app.sessions.sessions();
    let count = sessions.len();

    match key.code {
        KeyCode::Esc => {
            app.screen = Screen::ConnectionDialog;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if count > 0 {
                app.session_list_index = app.session_list_index.saturating_sub(1);
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if count > 0 {
                app.session_list_index = (app.session_list_index + 1).min(count - 1);
            }
        }
        KeyCode::Enter => {
            app.load_session(app.session_list_index);
        }
        KeyCode::Delete | KeyCode::Char('d') => {
            if count > 0 {
                app.delete_selected_session();
            }
        }
        _ => {}
    }
    false
}

/// Render the current TUI frame.
pub fn render(app: &mut App, frame: &mut Frame) {
    match app.screen {
        Screen::ConnectionDialog => render_connection_dialog(app, frame),
        Screen::SessionList => render_session_list(app, frame),
    }
}

fn render_connection_dialog(app: &mut App, frame: &mut Frame) {
    let area = centered_rect(60, 60, frame.area());

    let block = Block::default()
        .title(" Sutty — SSH Client ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    // Build the form content
    let mut lines: Vec<Line> = Vec::new();

    // Help line
    lines.push(Line::from(vec![
        Span::styled(
            " Enter ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("to connect  |  "),
        Span::styled("Ctrl+S", Style::default().fg(Color::Yellow)),
        Span::raw(" save  |  "),
        Span::styled("Ctrl+L", Style::default().fg(Color::Yellow)),
        Span::raw(" saved sessions  |  "),
        Span::styled("Esc", Style::default().fg(Color::Red)),
        Span::raw(" quit"),
    ]));
    lines.push(Line::from(""));

    // Editable fields
    lines.push(field_line(
        "   Hostname: ",
        &app.host,
        app.field == Field::Host,
        app,
    ));
    lines.push(field_line(
        "       Port: ",
        &app.port_str,
        app.field == Field::Port,
        app,
    ));
    lines.push(field_line(
        "   Username: ",
        &app.username,
        app.field == Field::Username,
        app,
    ));
    lines.push(field_line(
        "   Password: ",
        &mask_str(&app.password),
        app.field == Field::Password,
        app,
    ));
    lines.push(field_line(
        "   Key File: ",
        &app.key_file,
        app.field == Field::KeyFile,
        app,
    ));
    lines.push(Line::from(""));
    lines.push(field_line(
        "Session Name: ",
        &app.session_name,
        app.field == Field::SessionName,
        app,
    ));

    // Status line
    if !app.status_msg.is_empty() {
        lines.push(Line::from(""));
        let style = if app.status_is_error {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::Green)
        };
        lines.push(Line::from(Span::styled(
            format!("  {}", app.status_msg),
            style,
        )));
    }

    let paragraph = Paragraph::new(Text::from(lines))
        .block(block)
        .wrap(Wrap { trim: false });

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

fn render_session_list(app: &mut App, frame: &mut Frame) {
    let area = centered_rect(50, 60, frame.area());

    let sessions = app.sessions.sessions();
    let items: Vec<ListItem> = sessions
        .iter()
        .map(|s| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    &s.name,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!("  {}@{}:{}", s.username, s.host, s.port)),
            ]))
        })
        .collect();

    let mut list_state = ListState::default();
    if !items.is_empty() {
        list_state.select(Some(app.session_list_index));
    }

    let list = List::new(items)
        .block(
            Block::default()
                .title(" Saved Sessions ")
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    // Help bar
    let help = Paragraph::new(Line::from(vec![
        Span::styled(
            "Enter",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" load  |  "),
        Span::styled("D/Delete", Style::default().fg(Color::Red)),
        Span::raw(" delete  |  "),
        Span::styled("Esc", Style::default().fg(Color::Yellow)),
        Span::raw(" back"),
    ]))
    .alignment(Alignment::Center);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(2)])
        .split(area);

    frame.render_widget(Clear, area);
    frame.render_stateful_widget(list, layout[0], &mut list_state);
    frame.render_widget(help, layout[1]);
}

/// Build a labelled form field line.
fn field_line(label: &str, value: &str, focused: bool, _app: &App) -> Line<'static> {
    let label_span = Span::styled(label.to_string(), Style::default().fg(Color::White));
    let cursor = if focused { "▌" } else { " " };

    let display = if value.is_empty() && !focused {
        "(empty)".to_string()
    } else {
        format!("{}{}", value, cursor)
    };

    let value_style = if focused {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };

    Line::from(vec![label_span, Span::styled(display, value_style)])
}

/// Mask a password string for display.
fn mask_str(s: &str) -> String {
    "*".repeat(s.len())
}

/// Create a rectangle centered within `r` with the given width/height percentages.
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
