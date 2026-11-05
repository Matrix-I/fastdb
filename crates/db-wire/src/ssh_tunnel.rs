use crate::traits::SshTunnelConfig;
use anyhow::{Context, Result, ensure};
use async_trait::async_trait;
use russh::client::{self, Handler};
use russh_keys::key::PublicKey;
use std::{io::Read, sync::Arc, time::Duration};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::JoinSet,
    time::timeout,
};

const SETUP_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CHANNELS: usize = 16;
const MAX_PRIVATE_KEY_BYTES: u64 = 64 * 1024;

struct ClientHandler {
    fingerprint: String,
    _session_closed: oneshot::Sender<()>,
}

#[async_trait]
impl Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(&mut self, key: &PublicKey) -> Result<bool, Self::Error> {
        Ok(self.fingerprint == format!("SHA256:{}", key.fingerprint()))
    }
}

pub struct SshTunnel {
    pub local_bound_port: u16,
    // Dropping the sender wakes the listener even when all channel slots are busy.
    _shutdown: oneshot::Sender<()>,
}

struct SocketShutdown(std::net::TcpStream);

impl Drop for SocketShutdown {
    fn drop(&mut self) {
        let _ = self.0.shutdown(std::net::Shutdown::Both);
    }
}

struct SshSession {
    client: client::Handle<ClientHandler>,
    closed: oneshot::Receiver<()>,
    _socket: SocketShutdown,
}

fn load_private_key(path: &str, passphrase: Option<&str>) -> Result<russh_keys::key::KeyPair> {
    ensure!(
        std::fs::metadata(path)?.is_file(),
        "SSH key must be a regular file"
    );
    let file = std::fs::File::open(path)?;
    ensure!(file.metadata()?.is_file(), "SSH key must be a regular file");
    let mut contents = String::new();
    file.take(MAX_PRIVATE_KEY_BYTES + 1)
        .read_to_string(&mut contents)?;
    ensure!(
        contents.len() as u64 <= MAX_PRIVATE_KEY_BYTES,
        "SSH private key is too large"
    );
    Ok(russh_keys::decode_secret_key(&contents, passphrase)?)
}

impl SshTunnelConfig {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.host.is_empty() && self.port != 0 && !self.user.is_empty(),
            "SSH host, port and username are required"
        );
        ensure!(
            self.host_key_fingerprint
                .strip_prefix("SHA256:")
                .is_some_and(|digest| {
                    digest.len() == 43
                        && digest
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"+/".contains(&byte))
                }),
            "Enter the trusted SSH host key's SHA256 fingerprint"
        );
        ensure!(
            self.key_path.is_some() || self.password.is_some(),
            "SSH requires a private key or password"
        );

        Ok(())
    }
}

async fn connect(config: &SshTunnelConfig) -> Result<SshSession> {
    config.validate()?;

    let key = if let Some(path) = &config.key_path {
        let path = path.clone();
        let passphrase = config.password.clone();
        Some(
            timeout(
                SETUP_TIMEOUT,
                tokio::task::spawn_blocking(move || load_private_key(&path, passphrase.as_deref())),
            )
            .await
            .context("SSH private key loading timed out")?
            .context("SSH private key worker failed")?
            .map_err(|_| {
                anyhow::anyhow!("Could not load SSH private key; check the file and passphrase")
            })?,
        )
    } else {
        None
    };
    timeout(SETUP_TIMEOUT, async {
        let (session_closed, closed) = oneshot::channel();
        let handler = ClientHandler {
            fingerprint: config.host_key_fingerprint.clone(),
            _session_closed: session_closed,
        };
        let socket = TcpStream::connect((config.host.as_str(), config.port))
            .await
            .context("Could not reach SSH host; check the address and port")?
            .into_std()?;
        // russh runs key exchange in a detached task. Shut down its TCP socket
        // independently if setup times out or the owning forwarder is dropped.
        let shutdown = SocketShutdown(socket.try_clone()?);
        let mut session = client::connect_stream(
            Arc::new(client::Config::default()),
            TcpStream::from_std(socket)?,
            handler,
        )
        .await
        .map_err(|error| match error {
            russh::Error::UnknownKey => {
                anyhow::anyhow!("SSH host key does not match the trusted fingerprint")
            }
            error => anyhow::anyhow!("Could not establish SSH connection: {error}"),
        })?;
        let accepted = if let Some(key) = key {
            session
                .authenticate_publickey(&config.user, Arc::new(key))
                .await
        } else {
            session
                .authenticate_password(&config.user, config.password.as_deref().unwrap_or_default())
                .await
        }
        .context("SSH authentication exchange failed")?;
        ensure!(
            accepted,
            "SSH authentication rejected; check the username and credentials"
        );
        Ok(SshSession {
            client: session,
            closed,
            _socket: shutdown,
        })
    })
    .await
    .context("SSH connection or authentication timed out")?
}

impl SshTunnel {
    pub(crate) fn is_closed(&self) -> bool {
        self._shutdown.is_closed()
    }

    pub async fn test_connection(config: &SshTunnelConfig) -> Result<()> {
        let session = connect(config).await?;
        disconnect(&session.client).await;
        Ok(())
    }

    pub async fn start_forwarding(
        config: SshTunnelConfig,
        remote_host: String,
        remote_port: u32,
    ) -> Result<Self> {
        crate::run_on_shared_runtime(Self::start(config, remote_host, remote_port)).await
    }

    async fn start(config: SshTunnelConfig, remote_host: String, remote_port: u32) -> Result<Self> {
        ensure!(
            !remote_host.is_empty() && (1..=u32::from(u16::MAX)).contains(&remote_port),
            "SSH forwarding requires a database host and valid port"
        );
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .context("Could not bind SSH forwarder")?;
        let local_bound_port = listener.local_addr()?.port();
        let SshSession {
            client,
            mut closed,
            _socket: socket,
        } = connect(&config).await?;
        let session = Arc::new(client);
        let (shutdown, mut stopped) = oneshot::channel();
        tokio::spawn(async move {
            let _socket = socket;
            let mut channels = JoinSet::new();
            loop {
                tokio::select! {
                    biased;
                    _ = &mut stopped => break,
                    _ = &mut closed => break,
                    finished = channels.join_next(), if !channels.is_empty() => {
                        if !matches!(finished, Some(Ok(true))) {
                            break;
                        }
                    }
                    incoming = listener.accept(), if channels.len() < MAX_CHANNELS => {
                        let Ok((mut socket, peer)) = incoming else { break };
                        let session = session.clone();
                        let host = remote_host.clone();
                        channels.spawn(async move {
                            let opened = timeout(SETUP_TIMEOUT, session.channel_open_direct_tcpip(
                                host, remote_port, peer.ip().to_string(), u32::from(peer.port()),
                            )).await;
                            match opened {
                                Ok(Ok(channel)) => forward_channel(&mut socket, channel).await,
                                Ok(Err(_)) => true,
                                // russh retains pending opens after the caller times out.
                                Err(_) => false,
                            }
                        });
                    }
                }
            }
            drop(listener);
            channels.shutdown().await;
            disconnect(&session).await;
        });
        Ok(Self {
            local_bound_port,
            _shutdown: shutdown,
        })
    }
}

async fn forward_channel(socket: &mut TcpStream, mut channel: russh::Channel<client::Msg>) -> bool {
    {
        let (mut local_read, mut local_write) = socket.split();
        let mut remote_write = channel.make_writer();
        let mut remote_read = channel.make_reader();
        let _ = tokio::try_join!(
            async {
                tokio::io::copy(&mut local_read, &mut remote_write).await?;
                remote_write.shutdown().await
            },
            async {
                tokio::io::copy(&mut remote_read, &mut local_write).await?;
                local_write.shutdown().await
            },
        );
    }
    matches!(timeout(SETUP_TIMEOUT, channel.close()).await, Ok(Ok(())))
}

async fn disconnect(session: &client::Handle<ClientHandler>) {
    let _ = timeout(
        SETUP_TIMEOUT,
        session.disconnect(russh::Disconnect::ByApplication, "", "en"),
    )
    .await;
}

#[cfg(test)]
#[path = "ssh_tunnel_tests.rs"]
mod tests;
