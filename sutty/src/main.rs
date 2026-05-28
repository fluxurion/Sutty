//! Sutty — A PuTTY-like SSH client written in Rust.
//!
//! Usage:
//!   sutty                           # Launch the TUI connection dialog
//!   sutty user@host                 # Quick connect with password prompt
//!   sutty -h host -u user -p 2222   # Connect with CLI args
//!   sutty --session mybox           # Connect using a saved session

mod terminal;
mod tui;

use anyhow::Result;
use clap::Parser;
use crossterm::event::{KeyCode, KeyModifiers};
use sutty_core::{config, ssh};
use tui::app::{App, DialogResult};

/// Sutty — a PuTTY-like SSH client in Rust.
#[derive(Parser, Debug)]
#[command(name = "sutty", version, about, long_about = None)]
struct Cli {
    /// Remote hostname or IP address
    #[arg(short = 'H', long)]
    host: Option<String>,

    /// SSH port (default: 22)
    #[arg(short = 'p', long, default_value = "22")]
    port: u16,

    /// Username for authentication
    #[arg(short = 'u', long)]
    username: Option<String>,

    /// Password (insecure; prefer key-based auth)
    #[arg(short = 'P', long)]
    password: Option<String>,

    /// Path to SSH private key file
    #[arg(short = 'i', long)]
    key_file: Option<String>,

    /// Connect using a saved session by name
    #[arg(short = 's', long)]
    session: Option<String>,

    /// Quick-connect string in user@host[:port] format (positional)
    #[arg()]
    destination: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let cli = Cli::parse();

    // Resolve connection parameters: CLI args > saved session > TUI dialog
    let (host, port, username, password, key_file) = resolve_connection_params(&cli).await?;

    // Run the SSH session
    run_ssh_session(&host, port, &username, password, key_file.as_deref()).await
}

/// Resolve connection params from CLI args, saved sessions, or the TUI dialog.
async fn resolve_connection_params(
    cli: &Cli,
) -> Result<(String, u16, String, Option<String>, Option<String>)> {
    // 1. Try saved session
    if let Some(ref session_name) = cli.session {
        let mgr = config::SessionManager::load()?;
        if let Some(cfg) = mgr.find(session_name) {
            return Ok((
                cfg.host.clone(),
                cfg.port,
                cfg.username.clone(),
                None, // password not stored in config
                cfg.key_file.clone(),
            ));
        }
        anyhow::bail!("Session '{}' not found", session_name);
    }

    // 2. Parse destination string (user@host[:port])
    if let Some(ref dest) = cli.destination {
        let (user_host, dest_port) = if let Some((uh, p)) = dest.rsplit_once(':') {
            (uh, Some(p.parse::<u16>()?))
        } else {
            (dest.as_str(), None)
        };

        let (user, host_part) = if let Some((u, h)) = user_host.split_once('@') {
            (u.to_string(), h.to_string())
        } else {
            // No @ — treat whole thing as host
            (
                cli.username.clone().unwrap_or_else(|| whoami::username()),
                user_host.to_string(),
            )
        };

        let port = dest_port
            .or(if cli.port != 22 { Some(cli.port) } else { None })
            .unwrap_or(22);
        let username = cli.username.as_ref().unwrap_or(&user).clone();
        let password = cli.password.clone();
        let key_file = cli.key_file.clone();

        return Ok((host_part, port, username, password, key_file));
    }

    // 3. Use explicit CLI flags
    if let Some(ref host) = cli.host {
        let username = cli.username.clone().unwrap_or_else(|| whoami::username());
        return Ok((
            host.clone(),
            cli.port,
            username,
            cli.password.clone(),
            cli.key_file.clone(),
        ));
    }

    // 4. No args — launch TUI connection dialog
    run_tui_dialog().await
}

/// Launch the ratatui-based connection dialog (PuTTY-like).
async fn run_tui_dialog() -> Result<(String, u16, String, Option<String>, Option<String>)> {
    let sessions = config::SessionManager::load()?;
    let mut app = App::new(sessions);

    // Setup terminal for TUI
    let mut stdout = std::io::stdout();
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::cursor::Hide
    )?;

    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    // Event loop
    loop {
        terminal.draw(|frame| tui::connection::render(&mut app, frame))?;

        if let crossterm::event::Event::Key(key) = crossterm::event::read()? {
            if tui::connection::handle_key(&mut app, key) {
                break;
            }
        }
    }

    // Restore terminal
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::cursor::Show
    )?;

    match app.result {
        Some(DialogResult::Connect {
            host,
            port,
            username,
            password,
            key_file,
        }) => Ok((host, port, username, Some(password), key_file)),
        Some(DialogResult::Quit) => {
            println!("Goodbye.");
            std::process::exit(0);
        }
        None => {
            println!("Goodbye.");
            std::process::exit(0);
        }
    }
}

/// Run the actual SSH session — raw terminal I/O loop.
async fn run_ssh_session(
    host: &str,
    port: u16,
    username: &str,
    password: Option<String>,
    key_file: Option<&str>,
) -> Result<()> {
    // Connect and start session
    let mut ssh = ssh::SshSession::connect(host, port, username, password, key_file).await?;

    let (cols, rows) = terminal::terminal_size()?;
    ssh.request_pty(cols as u32, rows as u32, "xterm-256color")
        .await?;

    log::info!("Connected to {}@{}:{}", username, host, port);

    // Enter raw terminal mode for the session
    terminal::enter_raw_mode()?;

    // Sniff the TERM_PROGRAM if we're running inside a modern terminal
    let result = session_loop(&mut ssh).await;

    // Always restore the terminal
    terminal::exit_raw_mode()?;
    ssh.close().await?;

    result
}

/// Main I/O loop: read local keystrokes → send to SSH, read SSH data → print to terminal.
async fn session_loop(ssh: &mut ssh::SshSession) -> Result<()> {
    use crossterm::event::{Event, KeyEventKind};
    use std::io::Write;

    let mut stdout = std::io::stdout();

    loop {
        // Poll for local keyboard input OR remote SSH data
        tokio::select! {
            // Local terminal event (key press or resize)
            _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {
                // Check for pending crossterm events
                while crossterm::event::poll(std::time::Duration::from_millis(0))? {
                    match crossterm::event::read()? {
                        Event::Key(key) if key.kind != KeyEventKind::Release => {
                            // Ctrl+D on its own? Send EOF-like signal (but don't exit — let the shell handle it)
                            // Ctrl+\ (1C) is the traditional "quit" signal
                            if key.code == KeyCode::Char('\\')
                                && key.modifiers.contains(KeyModifiers::CONTROL)
                            {
                                // Ctrl+\ → force disconnect (like OpenSSH ~.)
                                return Ok(());
                            }

                            let bytes = terminal::key_event_to_bytes(&key);
                            if !bytes.is_empty() {
                                ssh.send_data(&bytes).await?;
                            }
                        }
                        Event::Resize(w, h) => {
                            ssh.resize_pty(w as u32, h as u32).await?;
                        }
                        _ => {}
                    }
                }
            }

            // Remote SSH data
            event = ssh.receive() => {
                match event {
                    Some(ssh::SshEvent::Data(data)) => {
                        stdout.write_all(&data)?;
                        stdout.flush()?;
                    }
                    Some(ssh::SshEvent::Eof) | Some(ssh::SshEvent::ExitStatus(_)) => {
                        return Ok(());
                    }
                    None => return Ok(()),
                }
            }
        }
    }
}
