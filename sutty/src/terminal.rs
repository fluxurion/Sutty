//! Raw terminal mode handling — enter/exit raw mode and read input events.
//! On Unix we use `libc`; on Windows we use `windows-sys`.

use anyhow::Result;
use crossterm::{
    cursor,
    event::{KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{self, ClearType},
};

/// Put the terminal into raw mode and clear the screen, ready for the SSH session.
pub fn enter_raw_mode() -> Result<()> {
    terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, terminal::Clear(ClearType::All), cursor::Hide)?;
    Ok(())
}

/// Restore the terminal to its normal state.
pub fn exit_raw_mode() -> Result<()> {
    let mut stdout = std::io::stdout();
    execute!(stdout, cursor::Show)?;
    terminal::disable_raw_mode()?;
    Ok(())
}

/// Convert a crossterm KeyEvent into bytes suitable for sending over SSH.
/// Handles special keys like Enter, Tab, Backspace, arrows, etc.
pub fn key_event_to_bytes(event: &KeyEvent) -> Vec<u8> {
    match event.code {
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => vec![0x1b, b'[', b'A'],
        KeyCode::Down => vec![0x1b, b'[', b'B'],
        KeyCode::Right => vec![0x1b, b'[', b'C'],
        KeyCode::Left => vec![0x1b, b'[', b'D'],
        KeyCode::Home => vec![0x1b, b'[', b'H'],
        KeyCode::End => vec![0x1b, b'[', b'F'],
        KeyCode::PageUp => vec![0x1b, b'[', b'5', b'~'],
        KeyCode::PageDown => vec![0x1b, b'[', b'6', b'~'],
        KeyCode::Delete => vec![0x1b, b'[', b'3', b'~'],
        KeyCode::Insert => vec![0x1b, b'[', b'2', b'~'],
        KeyCode::F(n) => {
            // F1-F4: ESC O P-Q-R-S, F5+: ESC [ 1 5+n ~
            if n <= 4 {
                vec![0x1b, b'O', b'P' + (n as u8) - 1]
            } else if n <= 12 {
                let c = match n {
                    5 => b'1',
                    6 => b'1',
                    7 => b'1',
                    8 => b'1',
                    9 => b'2',
                    10 => b'2',
                    11 => b'2',
                    12 => b'2',
                    _ => unreachable!(),
                };
                let d = match n {
                    5 => b'5',
                    6 => b'7',
                    7 => b'8',
                    8 => b'9',
                    9 => b'0',
                    10 => b'1',
                    11 => b'3',
                    12 => b'4',
                    _ => unreachable!(),
                };
                vec![0x1b, b'[', c, d, b'~']
            } else {
                vec![]
            }
        }
        KeyCode::Char(c) => {
            // Handle Ctrl+letter combinations
            if event.modifiers.contains(KeyModifiers::CONTROL) && c.is_ascii_alphabetic() {
                let code = (c.to_ascii_lowercase() as u8) - b'a' + 1;
                vec![code]
            } else {
                let mut buf = [0u8; 4];
                c.encode_utf8(&mut buf);
                buf[..c.len_utf8()].to_vec()
            }
        }
        _ => vec![],
    }
}

/// Get terminal size (columns, rows).
pub fn terminal_size() -> Result<(u16, u16)> {
    let (w, h) = terminal::size()?;
    Ok((w, h))
}
