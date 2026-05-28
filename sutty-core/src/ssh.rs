//! SSH connection handling via the `russh` crate (pure-Rust SSHv2).

use anyhow::{Context, Result};
use async_trait::async_trait;
use russh::client::{Handle, Msg};
use russh::keys::PrivateKeyWithHashAlg;
use russh::*;
use std::future::Future;
use std::sync::Arc;

/// Data received from the remote session.
#[derive(Debug)]
pub enum SshEvent {
    Data(Vec<u8>),
    Eof,
    #[allow(dead_code)]
    ExitStatus(u32),
}

/// An SSH session that bridges local terminal ↔ remote shell.
pub struct SshSession {
    session: Handle<ClientHandler>,
    channel: Channel<Msg>,
}

impl SshSession {
    /// Connect to `host:port` as `username`, authenticating with a password
    /// prompt or with the given key file.
    pub async fn connect(
        host: &str,
        port: u16,
        username: &str,
        password: Option<String>,
        key_file: Option<&str>,
    ) -> Result<Self> {
        let config = Arc::new(client::Config::default());
        let sh = ClientHandler {};

        let addr = format!("{}:{}", host, port);
        let mut session = client::connect(config, &addr, sh)
            .await
            .with_context(|| format!("Failed to connect to {}", addr))?;

        let auth_result = if let Some(key_path) = key_file {
            let key = russh::keys::load_secret_key(key_path, None)
                .with_context(|| format!("Failed to load key from {}", key_path))?;
            let key_with_hash = PrivateKeyWithHashAlg::new(Arc::new(key), None);
            session
                .authenticate_publickey(username, key_with_hash)
                .await
                .with_context(|| format!("Key auth failed for {}", username))?
        } else if let Some(pass) = password {
            session
                .authenticate_password(username, &pass)
                .await
                .with_context(|| format!("Password auth failed for {}", username))?
        } else {
            anyhow::bail!("No authentication method provided (password or key file)")
        };

        if !auth_result.success() {
            anyhow::bail!("Authentication rejected by server");
        }

        let channel = session
            .channel_open_session()
            .await
            .context("Failed to open session channel")?;

        Ok(Self { session, channel })
    }

    /// Request a PTY with the given terminal dimensions and start a shell.
    pub async fn request_pty(&mut self, cols: u32, rows: u32, term: &str) -> Result<()> {
        self.channel
            .request_pty(false, term, cols, rows, 0, 0, &[])
            .await
            .context("Failed to request PTY")?;

        self.channel
            .exec(false, b"$SHELL -l")
            .await
            .context("Failed to exec shell")?;

        Ok(())
    }

    /// Resize the remote PTY.
    pub async fn resize_pty(&mut self, cols: u32, rows: u32) -> Result<()> {
        self.channel
            .window_change(cols, rows, 0, 0)
            .await
            .context("Failed to resize PTY")?;
        Ok(())
    }

    /// Send data (keystrokes) to the remote session.
    pub async fn send_data(&mut self, data: &[u8]) -> Result<()> {
        self.channel
            .data(data)
            .await
            .context("Failed to send data")?;
        Ok(())
    }

    /// Wait for the next data/event from the remote session.
    pub async fn receive(&mut self) -> Option<SshEvent> {
        loop {
            if let Some(msg) = self.channel.wait().await {
                match msg {
                    ChannelMsg::Data { ref data } => {
                        return Some(SshEvent::Data(data.to_vec()));
                    }
                    ChannelMsg::Eof => return Some(SshEvent::Eof),
                    ChannelMsg::ExitStatus { exit_status } => {
                        return Some(SshEvent::ExitStatus(exit_status));
                    }
                    _ => {}
                }
            } else {
                return None;
            }
        }
    }

    /// Close the SSH session gracefully.
    pub async fn close(self) -> Result<()> {
        self.channel.eof().await.ok();
        self.session
            .disconnect(Disconnect::ByApplication, "", "Client closing")
            .await
            .ok();
        Ok(())
    }
}

struct ClientHandler;

#[async_trait]
impl client::Handler for ClientHandler {
    type Error = anyhow::Error;

    fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send {
        std::future::ready(Ok(true))
    }
}
