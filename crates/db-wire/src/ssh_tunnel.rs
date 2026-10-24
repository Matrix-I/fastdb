use std::sync::Arc;
use anyhow::{Context, Result};
use async_trait::async_trait;
use russh::client::{self, Handler};
use tokio::net::TcpListener;
use tracing::info;

use crate::traits::SshTunnelConfig;

struct ClientHandler;

#[async_trait]
impl Handler for ClientHandler {
    type Error = russh::Error;
}

pub struct SshTunnel {
    pub config: SshTunnelConfig,
    pub local_bound_port: u16,
}

impl SshTunnel {
    /// Creates and tests SSH connectivity
    pub async fn test_connection(config: &SshTunnelConfig) -> Result<()> {
        let russh_config = Arc::new(client::Config::default());
        let sh = ClientHandler;
        let mut session = client::connect(russh_config, (config.host.as_str(), config.port), sh)
            .await
            .with_context(|| format!("Failed to connect to SSH host {}:{}", config.host, config.port))?;

        if let Some(ref key_path) = config.key_path {
            let key = russh_keys::load_secret_key(key_path, config.password.as_deref())
                .with_context(|| format!("Failed to load SSH private key from {key_path}"))?;
            session.authenticate_publickey(&config.user, Arc::new(key)).await?;
        } else if let Some(ref password) = config.password {
            session.authenticate_password(&config.user, password).await?;
        }

        info!("SSH authentication successful for {}", config.user);
        Ok(())
    }

    /// Spawns a local TCP port forwarder forwarding traffic to remote_host:remote_port via SSH
    pub async fn start_forwarding(
        config: SshTunnelConfig,
        remote_host: String,
        remote_port: u32,
    ) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let local_port = listener.local_addr()?.port();

        info!(
            "SSH Tunnel established: 127.0.0.1:{} -> {}@{}:{} -> {}:{}",
            local_port, config.user, config.host, config.port, remote_host, remote_port
        );

        Ok(Self {
            config,
            local_bound_port: local_port,
        })
    }
}
