use super::*;
use russh_keys::key::KeyPair;

#[test]
fn ssh_host_verification_accepts_only_the_pinned_key() {
    crate::get_runtime().block_on(async {
        let trusted = KeyPair::generate_ed25519().clone_public_key().unwrap();
        let untrusted = KeyPair::generate_ed25519().clone_public_key().unwrap();
        let mut handler = ClientHandler {
            fingerprint: format!("SHA256:{}", trusted.fingerprint()),
            _session_closed: oneshot::channel().0,
        };
        assert!(handler.check_server_key(&trusted).await.unwrap());
        assert!(!handler.check_server_key(&untrusted).await.unwrap());
    });
}

struct FixtureServer {
    database: Option<(String, u16, u32)>,
    client_key: Option<PublicKey>,
    tls: bool,
}

#[async_trait]
impl russh::server::Handler for FixtureServer {
    type Error = russh::Error;

    async fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> Result<russh::server::Auth, Self::Error> {
        Ok(if (user, password) == ("test", "fixture-password") {
            russh::server::Auth::Accept
        } else {
            russh::server::Auth::Reject {
                proceed_with_methods: None,
            }
        })
    }

    async fn auth_publickey(
        &mut self,
        user: &str,
        key: &PublicKey,
    ) -> Result<russh::server::Auth, Self::Error> {
        Ok(if user == "test" && self.client_key.as_ref() == Some(key) {
            russh::server::Auth::Accept
        } else {
            russh::server::Auth::Reject {
                proceed_with_methods: None,
            }
        })
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: russh::Channel<russh::server::Msg>,
        host: &str,
        port: u32,
        _: &str,
        _: u32,
        _: &mut russh::server::Session,
    ) -> Result<bool, Self::Error> {
        let known_host = if self.tls {
            matches!(host, "localhost" | "127.0.0.1")
        } else {
            host == "database.internal"
        };
        if !known_host || port != self.database.as_ref().map_or(5432, |db| db.2) {
            return Ok(false);
        }
        let database = match &self.database {
            Some((host, port, _)) => Some(TcpStream::connect((host.as_str(), *port)).await?),
            None => None,
        };
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut stream = channel.into_stream();
            if let Some(mut database) = database {
                let _ = tokio::io::copy_bidirectional(&mut stream, &mut database).await;
                return;
            }
            let mut bytes = [0; 5];
            if stream.read_exact(&mut bytes).await.is_ok() {
                let _ = stream.write_all(&bytes).await;
            }
        });
        Ok(true)
    }
}

struct KeyFile(std::path::PathBuf);

impl Drop for KeyFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn ssh_forwarding_authenticates_and_closes_owned_connections() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    crate::get_runtime().block_on(async {
        timeout(SETUP_TIMEOUT, async {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let key = KeyPair::generate_ed25519();
            let fingerprint = key.clone_public_key().unwrap().fingerprint();
            let mut settings = SshTunnelConfig {
                host: "127.0.0.1".into(),
                port: listener.local_addr().unwrap().port(),
                user: "test".into(),
                host_key_fingerprint: format!("SHA256:{fingerprint}"),
                key_path: None,
                password: Some("wrong-password".into()),
            };
            let server = Arc::new(russh::server::Config {
                keys: vec![key],
                auth_rejection_time: Duration::ZERO,
                ..Default::default()
            });
            let private_key = KeyPair::generate_ed25519();
            let public_key = private_key.clone_public_key().unwrap();
            let path =
                std::env::temp_dir().join(format!("fastdb-ssh-{}.pem", uuid::Uuid::new_v4()));
            let key_file = KeyFile(path);
            let mut options = std::fs::OpenOptions::new();
            options.create_new(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let file = options.open(&key_file.0).unwrap();
            russh_keys::encode_pkcs8_pem(&private_key, file).unwrap();
            let server_task = tokio::spawn(async move {
                for _ in 0..4 {
                    let (socket, _) = listener.accept().await.unwrap();
                    let connection = russh::server::run_stream(
                        server.clone(),
                        socket,
                        FixtureServer {
                            database: None,
                            client_key: Some(public_key.clone()),
                            tls: false,
                        },
                    )
                    .await
                    .unwrap();
                    let _ = connection.await;
                }
            });
            let mut wrong_pin = settings.clone();
            wrong_pin.host_key_fingerprint = format!("SHA256:{}", "A".repeat(43));
            let error = SshTunnel::test_connection(&wrong_pin).await.unwrap_err();
            assert!(error.to_string().contains("SSH host key does not match"));

            let error = SshTunnel::test_connection(&settings).await.unwrap_err();
            assert!(error.to_string().contains("SSH authentication rejected"));

            let mut key_settings = settings.clone();
            key_settings.password = None;
            key_settings.key_path = Some(key_file.0.to_str().unwrap().into());
            SshTunnel::test_connection(&key_settings).await.unwrap();

            settings.password = Some("fixture-password".into());
            let tunnel = SshTunnel::start_forwarding(settings, "database.internal".into(), 5432)
                .await
                .unwrap();
            let address = ("127.0.0.1", tunnel.local_bound_port);
            let mut stream = TcpStream::connect(address).await.unwrap();
            stream.write_all(b"hello").await.unwrap();
            let mut reply = [0; 5];
            stream.read_exact(&mut reply).await.unwrap();
            assert_eq!(&reply, b"hello");
            drop(tunnel);
            server_task.await.unwrap();
            assert!(TcpStream::connect(address).await.is_err());
        })
        .await
        .expect("SSH fixture did not finish");
    });
}

#[test]
fn ssh_postgres_routes_main_and_isolated_sessions() {
    check_database_through_ssh(
        crate::DbDriverKind::Postgres,
        "PG",
        "postgres",
        "postgres",
        false,
    );
}

#[test]
fn ssh_mysql_routes_pool_and_isolated_sessions() {
    check_database_through_ssh(crate::DbDriverKind::MySql, "MYSQL", "mysql", "root", false);
}

#[test]
fn ssh_tls_postgres_preserves_server_identity() {
    check_database_through_ssh(
        crate::DbDriverKind::Postgres,
        "PG",
        "postgres",
        "postgres",
        true,
    );
}

#[test]
fn ssh_tls_mysql_preserves_server_identity() {
    check_database_through_ssh(crate::DbDriverKind::MySql, "MYSQL", "mysql", "root", true);
}

fn check_database_through_ssh(
    kind: crate::DbDriverKind,
    prefix: &str,
    default_database: &str,
    default_user: &str,
    tls: bool,
) {
    use crate::ConnectionConfig;
    let env_group = if tls { "TLS" } else { "TEST" };
    let required = if tls {
        "FASTDB_REQUIRE_TLS_TESTS"
    } else {
        "FASTDB_REQUIRE_DB_TESTS"
    };
    let setting = |key| std::env::var(format!("FASTDB_{env_group}_{prefix}_{key}"));
    let Ok(port) = setting("PORT") else {
        assert!(
            std::env::var_os(required).is_none(),
            "{required} is set but FASTDB_{env_group}_{prefix}_PORT is not"
        );
        return;
    };
    let remote_port = if tls { 15432 } else { kind.default_port() };
    let database = (
        setting("HOST").unwrap_or_else(|_| "127.0.0.1".into()),
        port.parse::<u16>()
            .expect("Database fixture port must be a port number"),
        u32::from(remote_port),
    );
    crate::get_runtime().block_on(async {
        timeout(SETUP_TIMEOUT, async {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let key = KeyPair::generate_ed25519();
            let fingerprint = key.clone_public_key().unwrap().fingerprint();
            let settings = SshTunnelConfig {
                host: "127.0.0.1".into(),
                port: listener.local_addr().unwrap().port(),
                user: "test".into(),
                host_key_fingerprint: format!("SHA256:{fingerprint}"),
                key_path: None,
                password: Some("fixture-password".into()),
            };
            let server = Arc::new(russh::server::Config {
                keys: vec![key],
                ..Default::default()
            });
            let server_task = tokio::spawn(async move {
                let mut sessions = JoinSet::new();
                for _ in 0..if tls { 4 } else { 2 } {
                    let (socket, _) = listener.accept().await.unwrap();
                    let server = server.clone();
                    let database = Some(database.clone());
                    sessions.spawn(async move {
                        let session = russh::server::run_stream(
                            server,
                            socket,
                            FixtureServer {
                                database,
                                client_key: None,
                                tls,
                            },
                        )
                        .await
                        .unwrap();
                        let _ = session.await;
                    });
                }
                while let Some(session) = sessions.join_next().await {
                    session.unwrap();
                }
            });
            let config = ConnectionConfig {
                driver: kind,
                host: if tls {
                    "localhost"
                } else {
                    "database.internal"
                }
                .into(),
                port: remote_port,
                database: setting("DATABASE").unwrap_or_else(|_| default_database.into()),
                user: setting("USER").unwrap_or_else(|_| default_user.into()),
                password: setting("PASSWORD").ok().or_else(|| {
                    (tls && kind == crate::DbDriverKind::MySql).then(|| "fastdb".into())
                }),
                ssh_tunnel: Some(settings),
                tls_enabled: tls,
                tls_ca_cert: tls
                    .then(|| std::env::var("FASTDB_TLS_CA").expect("FASTDB_TLS_CA required")),
                ..Default::default()
            };
            if tls {
                let wrong_host = ConnectionConfig {
                    host: "127.0.0.1".into(),
                    ..config.clone()
                };
                assert!(
                    crate::create_driver(wrong_host)
                        .unwrap()
                        .ping()
                        .await
                        .is_err()
                );
                let untrusted = ConnectionConfig {
                    tls_ca_cert: None,
                    ..config.clone()
                };
                assert!(
                    crate::create_driver(untrusted)
                        .unwrap()
                        .ping()
                        .await
                        .is_err()
                );
            }
            let driver = crate::create_driver(config).unwrap();
            let (pager, _, _) = driver.execute_query("SELECT 42").await.unwrap();
            assert_eq!(pager.get_cell(0, 0).unwrap(), b"42");
            let session = driver.open_script_session().await.unwrap();
            let (events, results) = flume::bounded(16);
            session
                .execute_streaming("SELECT 73", events)
                .await
                .unwrap();
            let cell = results.try_iter().find_map(|event| match event {
                crate::StreamEvent::SchemaReady(pager) => pager.get_cell(0, 0),
                _ => None,
            });
            assert_eq!(cell.as_deref(), Some(b"73".as_slice()));
            session.finish().await.unwrap();
            drop(driver);
            server_task.await.unwrap();
        })
        .await
        .expect("Database SSH fixture did not finish");
    });
}
