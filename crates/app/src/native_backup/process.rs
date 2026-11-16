use super::BackupArtifact;
use anyhow::{Context, Result, ensure};
use std::fs::File;
use std::process::{Command, Stdio};
use std::time::Duration;
use tokio::io::{self, AsyncRead, AsyncReadExt};
use tokio::process::Child;
use tokio::sync::watch;

pub(super) const LOG_LIMIT: usize = 64 * 1024;
const REAP_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) enum BackupOutput {
    NativeFile(BackupArtifact),
    Stdout(BackupArtifact),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Completed,
    Canceled,
}

pub(crate) async fn run(
    mut command: Command,
    input: Option<File>,
    output: Option<BackupOutput>,
    mut cancel: watch::Receiver<bool>,
    logs: watch::Sender<String>,
    timeout: Duration,
) -> Result<Outcome> {
    if *cancel.borrow() || cancel.has_changed().is_err() {
        return Ok(Outcome::Canceled);
    }
    let stdout = match &output {
        Some(BackupOutput::Stdout(artifact)) => artifact.temporary.as_file().try_clone()?.into(),
        _ => Stdio::piped(),
    };
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = tokio::process::Command::from(command)
        .stdin(input.map_or_else(Stdio::null, Stdio::from))
        .stdout(stdout)
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("Cannot start the native database tool")?;
    let stderr = child
        .stderr
        .take()
        .context("Native tool has no error stream")?;
    let stdout = child.stdout.take();
    let result = tokio::select! {
        biased;
        _ = async { let _ = cancel.wait_for(|requested| *requested).await; } => {
            stop(&mut child).await?;
            return Ok(Outcome::Canceled);
        }
        result = tokio::time::timeout(timeout, async {
            // Keep the process-group leader unreaped while descendants may still
            // hold the pipes, so its ID cannot be reused before cancellation.
            capture(stdout, stderr, &logs).await?;
            child.wait().await
        }) => result,
    };
    let status = match result {
        Ok(Ok(status)) => status,
        Ok(Err(error)) => {
            stop(&mut child).await?;
            return Err(error).context("Cannot supervise the native database tool");
        }
        Err(_) => {
            stop(&mut child).await?;
            anyhow::bail!("Native database tool exceeded its time limit");
        }
    };
    ensure!(status.success(), "Native database tool failed ({status})");
    if let Some(BackupOutput::NativeFile(artifact) | BackupOutput::Stdout(artifact)) = output {
        tokio::task::spawn_blocking(move || artifact.publish()).await??;
    }
    Ok(Outcome::Completed)
}

async fn stop(child: &mut Child) -> Result<()> {
    tokio::time::timeout(REAP_TIMEOUT, async {
        #[cfg(unix)]
        if let Some(id) = child.id() {
            kill_process_group(i32::try_from(id).context("Invalid native process ID")?)?;
            child.wait().await?;
            return Ok::<_, anyhow::Error>(());
        }
        child.kill().await?;
        Ok(())
    })
    .await
    .context("Timed out waiting for the native tool to exit after cancellation")?
    .context("Cannot stop the native database tool")
}

#[cfg(unix)]
fn kill_process_group(group: i32) -> io::Result<()> {
    const SIGKILL: i32 = 9;
    const ESRCH: i32 = 3;
    unsafe extern "C" {
        fn killpg(pgrp: i32, signal: i32) -> i32;
    }
    // The group is a positive PID returned by spawn with process_group(0).
    // The leader remains unreaped until after signaling on cancellation.
    if unsafe { killpg(group, SIGKILL) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(ESRCH) {
        return Ok(());
    }
    Err(error)
}

async fn capture(
    mut stdout: Option<impl AsyncRead + Unpin>,
    mut stderr: impl AsyncRead + Unpin,
    logs: &watch::Sender<String>,
) -> io::Result<()> {
    let mut retained = Vec::new();
    let mut out = [0; 4096];
    let mut err = [0; 4096];
    let mut read_stderr = true;
    while stdout.is_some() || read_stderr {
        let chunk = tokio::select! {
            count = async { stdout.as_mut().unwrap().read(&mut out).await }, if stdout.is_some() => {
                match count? {
                    0 => { stdout = None; continue; }
                    count => &out[..count],
                }
            }
            count = stderr.read(&mut err), if read_stderr => {
                match count? {
                    0 => { read_stderr = false; continue; }
                    count => &err[..count],
                }
            }
        };
        let discard = (retained.len() + chunk.len()).saturating_sub(LOG_LIMIT);
        retained.drain(..discard);
        retained.extend_from_slice(chunk);
        let text = String::from_utf8_lossy(&retained);
        let start = text.ceil_char_boundary(text.len().saturating_sub(LOG_LIMIT));
        logs.send_replace(text[start..].to_owned());
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn cancellation_closes_a_wrappers_descendant_input() -> Result<()> {
        use std::os::fd::OwnedFd;
        use std::os::unix::net::UnixStream;

        db_wire::get_runtime().block_on(async {
            let (peer, input) = UnixStream::pair()?;
            peer.set_nonblocking(true)?;
            let mut peer = tokio::net::UnixStream::from_std(peer)?;
            let mut command = Command::new("/bin/sh");
            command.args([
                "-c",
                "exec 3<&0; /bin/sh -c 'printf ready >&2; exec /bin/cat >/dev/null' <&3 & wait",
            ]);
            let (cancel, receiver) = watch::channel(false);
            let (progress, mut logs) = watch::channel(String::new());
            let cancellation = async {
                logs.wait_for(|text| text == "ready").await?;
                cancel.send(true)?;
                Ok::<_, anyhow::Error>(())
            };
            let (outcome, cancellation) = tokio::join!(
                run(
                    command,
                    Some(File::from(OwnedFd::from(input))),
                    None,
                    receiver,
                    progress,
                    Duration::from_secs(5),
                ),
                cancellation
            );
            cancellation?;
            assert_eq!(outcome?, Outcome::Canceled);
            let mut byte = [0];
            let received = tokio::time::timeout(Duration::from_secs(2), peer.read(&mut byte))
                .await
                .context("The canceled wrapper left a descendant holding its input open")??;
            assert_eq!(received, 0);
            Ok(())
        })
    }

    #[test]
    fn reports_bounded_output_and_publishes_only_on_success() -> anyhow::Result<()> {
        db_wire::get_runtime().block_on(async {
            let directory = tempfile::tempdir()?;
            let destination = directory.path().join("verified");
            let artifact = BackupArtifact::stage(&destination)?;
            let mut command = Command::new("/bin/sh");
            command.args([
                "-c",
                "printf 'archive'; printf '%70000s' x >&2; printf 'finished' >&2",
            ]);
            let (_cancel, receiver) = watch::channel(false);
            let (progress, logs) = watch::channel(String::new());
            let outcome = run(
                command,
                None,
                Some(BackupOutput::Stdout(artifact)),
                receiver,
                progress,
                Duration::from_secs(5),
            )
            .await?;

            assert_eq!(outcome, Outcome::Completed);
            assert_eq!(std::fs::read(&destination)?, b"archive");
            assert!(logs.borrow().len() <= LOG_LIMIT);
            assert!(logs.borrow().ends_with("finished"));

            let failed = directory.path().join("failed");
            let artifact = BackupArtifact::stage(&failed)?;
            let mut command = Command::new("/bin/sh");
            command.args(["-c", "printf 'incomplete'; printf 'failure' >&2; exit 12"]);
            let (_cancel, receiver) = watch::channel(false);
            let (progress, logs) = watch::channel(String::new());
            let result = run(
                command,
                None,
                Some(BackupOutput::Stdout(artifact)),
                receiver,
                progress,
                Duration::from_secs(5),
            )
            .await;
            assert!(result.unwrap_err().to_string().contains("12"));
            assert!(!failed.exists());
            assert_eq!(&*logs.borrow(), "failure");
            Ok(())
        })
    }

    #[test]
    fn streams_a_file_through_stdin_to_native_path_output() -> anyhow::Result<()> {
        db_wire::get_runtime().block_on(async {
            let directory = tempfile::tempdir()?;
            let input = directory.path().join("input");
            std::fs::write(&input, b"fixture\0archive")?;
            let destination = directory.path().join("output");
            let artifact = BackupArtifact::stage(&destination)?;
            let mut command = Command::new("/bin/sh");
            command.args(["-c", "exec /bin/cat > \"$1\"", "fixture"]);
            command.arg(artifact.path());
            let (_cancel, receiver) = watch::channel(false);
            let (progress, _logs) = watch::channel(String::new());
            let outcome = run(
                command,
                Some(File::open(input)?),
                Some(BackupOutput::NativeFile(artifact)),
                receiver,
                progress,
                Duration::from_secs(5),
            )
            .await?;

            assert_eq!(outcome, Outcome::Completed);
            assert_eq!(std::fs::read(destination)?, b"fixture\0archive");
            Ok(())
        })
    }

    #[test]
    fn cancellation_after_child_starts_removes_partial_output() -> anyhow::Result<()> {
        db_wire::get_runtime().block_on(async {
            let directory = tempfile::tempdir()?;
            let destination = directory.path().join("canceled");
            let artifact = BackupArtifact::stage(&destination)?;
            let stage = artifact.path().to_owned();
            let mut command = Command::new("/bin/sh");
            command.args([
                "-c",
                "printf 'partial'; printf 'ready' >&2; while :; do :; done",
            ]);
            let (cancel, receiver) = watch::channel(false);
            let (progress, mut logs) = watch::channel(String::new());
            let cancellation = async {
                logs.wait_for(|text| text == "ready").await?;
                cancel.send(true)?;
                Ok::<_, anyhow::Error>(())
            };
            let (outcome, cancellation) = tokio::join!(
                run(
                    command,
                    None,
                    Some(BackupOutput::Stdout(artifact)),
                    receiver,
                    progress,
                    Duration::from_secs(5),
                ),
                cancellation,
            );

            cancellation?;
            assert_eq!(outcome?, Outcome::Canceled);
            assert!(!stage.exists());
            assert!(!destination.exists());
            Ok(())
        })
    }
}
