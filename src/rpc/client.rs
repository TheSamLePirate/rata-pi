//! `RpcClient` — the actor that owns the pi child process I/O.
//!
//! One writer task owns `ChildStdin`; one reader task owns `ChildStdout` and frames
//! incoming JSONL via [`JsonlCodec`]. A third task drains stderr into tracing logs
//! so the TTY never sees it.
//!
//! Responses (`{"type":"response",…}`) carrying an `id` are routed to the matching
//! `oneshot` waiter registered by [`RpcClient::call`]. Everything else flows out as
//! [`Incoming`] on the public `events` receiver.
//!
//! Shutdown: drop [`RpcClient`] → the command channel closes → the writer task
//! flushes and exits → child process receives EOF on stdin → reader and stderr
//! tasks drain and exit. The caller should additionally `wait()` the child.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use color_eyre::eyre::{Context, Result, eyre};
use futures::StreamExt;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_util::codec::FramedRead;

use super::codec::JsonlCodec;
use super::commands::{Envelope, ExtensionUiResponse, RpcCommand};
use super::events::Incoming;
use super::process::PiProcess;

/// Outcome of a correlated RPC call.
#[derive(Debug, Clone)]
pub struct RpcOk {
    /// The command name pi echoed back in its response. Surfaced for debug
    /// logging; callers typically only care about `data`.
    #[allow(dead_code)]
    pub command: String,
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    #[error("rpc send failed: {0}")]
    Send(String),
    #[error("rpc connection closed")]
    Closed,
    #[error("pi returned an error on {command}: {message}")]
    Remote { command: String, message: String },
    /// V3.a · per-call timeout elapsed before pi produced a response. The
    /// waiter is removed from `pending` so a late response is dropped
    /// cleanly. Callers typically surface this as a non-fatal flash rather
    /// than treating it as a session-ending error.
    #[error("rpc timed out after {0:?}")]
    Timeout(Duration),
}

/// Messages the writer task consumes.
enum OutMsg {
    Json(String),
    Shutdown,
}

/// Public handle used by the UI.
pub struct RpcClient {
    tx: mpsc::Sender<OutMsg>,
    pending: Pending,
    id_counter: AtomicU64,
    debug_rpc: bool,
}

type Pending = Arc<Mutex<HashMap<String, oneshot::Sender<Result<RpcOk, RpcError>>>>>;

/// What [`RpcClient::spawn`] returns alongside the client.
pub struct RpcIo {
    pub events: mpsc::Receiver<Incoming>,
    pub child: Child,
    pub tasks: Tasks,
}

pub struct Tasks {
    pub writer: JoinHandle<()>,
    pub reader: JoinHandle<()>,
    pub stderr: JoinHandle<()>,
}

impl RpcClient {
    /// Launch reader/writer/stderr tasks from a spawned [`PiProcess`].
    /// Returns the client plus an [`RpcIo`] bundle holding the event stream,
    /// the `Child` (for `wait()`ing on shutdown), and the task handles.
    pub fn spawn(pi: PiProcess, debug_rpc: bool) -> (Self, RpcIo) {
        let (out_tx, out_rx) = mpsc::channel::<OutMsg>(256);
        let (evt_tx, evt_rx) = mpsc::channel::<Incoming>(1024);
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));

        let writer = tokio::spawn(writer_task(pi.stdin, out_rx));
        let reader = tokio::spawn(reader_task(
            pi.stdout,
            evt_tx,
            Arc::clone(&pending),
            debug_rpc,
        ));
        let stderr = tokio::spawn(stderr_task(pi.stderr));

        let client = Self {
            tx: out_tx,
            pending,
            id_counter: AtomicU64::new(1),
            debug_rpc,
        };

        let io = RpcIo {
            events: evt_rx,
            child: pi.child,
            tasks: Tasks {
                writer,
                reader,
                stderr,
            },
        };
        (client, io)
    }

    fn next_id(&self) -> String {
        format!("req-{}", self.id_counter.fetch_add(1, Ordering::Relaxed))
    }

    /// Send a command and await its correlated response. Defaults to a 10 s
    /// bound so a single degraded RPC can never freeze the UI indefinitely —
    /// hot paths that want tighter bounds call [`RpcClient::call_timeout`]
    /// directly. Non-success responses surface as `Err(RpcError::Remote)`.
    pub async fn call(&self, command: RpcCommand) -> Result<RpcOk, RpcError> {
        self.call_timeout(command, Duration::from_secs(10)).await
    }

    /// Like [`RpcClient::call`] but with an explicit per-call timeout. On
    /// `Timeout` the pending waiter is removed so a late response from pi is
    /// dropped cleanly rather than leaking into the map.
    pub async fn call_timeout(
        &self,
        command: RpcCommand,
        timeout: Duration,
    ) -> Result<RpcOk, RpcError> {
        let id = self.next_id();
        let env = Envelope {
            id: Some(id.clone()),
            command: &command,
        };
        let json = env
            .to_json()
            .map_err(|e| RpcError::Send(format!("serialize: {e}")))?;

        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id.clone(), tx);

        if self.debug_rpc {
            tracing::debug!(rpc_out = %json);
        }
        // V3.a · if the writer channel is dead, yank the entry we just
        // inserted so repeated Closed errors can't accumulate dead waiters.
        if self.tx.send(OutMsg::Json(json)).await.is_err() {
            self.pending.lock().await.remove(&id);
            return Err(RpcError::Closed);
        }

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => {
                // Reader task exited and dropped the oneshot sender.
                self.pending.lock().await.remove(&id);
                Err(RpcError::Closed)
            }
            Err(_) => {
                // Bound elapsed. Evict the waiter; the reader will find no
                // match for the eventual response (if any) and discard it.
                self.pending.lock().await.remove(&id);
                Err(RpcError::Timeout(timeout))
            }
        }
    }

    /// Fire-and-forget; useful for `abort`, `set_editor_text`, etc. when the
    /// caller has no interest in the response.
    pub async fn fire(&self, command: RpcCommand) -> Result<(), RpcError> {
        let id = self.next_id();
        let env = Envelope {
            id: Some(id),
            command: &command,
        };
        let json = env
            .to_json()
            .map_err(|e| RpcError::Send(format!("serialize: {e}")))?;

        if self.debug_rpc {
            tracing::debug!(rpc_out = %json);
        }
        self.tx
            .send(OutMsg::Json(json))
            .await
            .map_err(|_| RpcError::Closed)
    }

    /// Reply to an `extension_ui_request`. Does not go through command
    /// correlation — the `id` field on the response matches the request, not
    /// one of our outgoing request ids. Consumed by the M4 extension UI router.
    pub async fn send_ext_ui_response(&self, resp: ExtensionUiResponse) -> Result<(), RpcError> {
        let json =
            serde_json::to_string(&resp).map_err(|e| RpcError::Send(format!("serialize: {e}")))?;
        if self.debug_rpc {
            tracing::debug!(rpc_out = %json);
        }
        self.tx
            .send(OutMsg::Json(json))
            .await
            .map_err(|_| RpcError::Closed)
    }

    /// Signal the writer task to flush and close pi's stdin.
    pub async fn shutdown(&self) {
        let _ = self.tx.send(OutMsg::Shutdown).await;
    }
}

// ─────────────────────────────────────────────────────────────── tasks ──

async fn writer_task(mut stdin: tokio::process::ChildStdin, mut rx: mpsc::Receiver<OutMsg>) {
    while let Some(msg) = rx.recv().await {
        match msg {
            OutMsg::Json(json) => {
                if let Err(e) = stdin.write_all(json.as_bytes()).await {
                    tracing::error!(error = %e, "rpc write failed");
                    return;
                }
                if let Err(e) = stdin.write_all(b"\n").await {
                    tracing::error!(error = %e, "rpc write newline failed");
                    return;
                }
                if let Err(e) = stdin.flush().await {
                    tracing::error!(error = %e, "rpc flush failed");
                    return;
                }
            }
            OutMsg::Shutdown => break,
        }
    }
    let _ = stdin.shutdown().await;
}

async fn reader_task(
    stdout: BufReader<tokio::process::ChildStdout>,
    evt_tx: mpsc::Sender<Incoming>,
    pending: Pending,
    debug_rpc: bool,
) {
    let mut framed = FramedRead::new(stdout, JsonlCodec::default());
    while let Some(next) = framed.next().await {
        let line = match next {
            Ok(l) => l,
            Err(e) => {
                tracing::error!(error = %e, "rpc read error");
                break;
            }
        };
        if debug_rpc {
            tracing::debug!(rpc_in = %line);
        }
        match serde_json::from_str::<Incoming>(&line) {
            Ok(msg) => dispatch(msg, &evt_tx, &pending).await,
            Err(e) => {
                tracing::warn!(error = %e, line = %line, "rpc parse failed");
            }
        }
    }
    tracing::info!("rpc reader task ended");
}

async fn dispatch(msg: Incoming, evt_tx: &mpsc::Sender<Incoming>, pending: &Pending) {
    match msg {
        Incoming::Response {
            id,
            command,
            success,
            error,
            data,
        } => {
            let Some(id) = id else {
                tracing::warn!(command = %command, "response without id");
                return;
            };
            let waiter = pending.lock().await.remove(&id);
            if let Some(tx) = waiter {
                let result = if success {
                    Ok(RpcOk {
                        command: command.clone(),
                        data,
                    })
                } else {
                    Err(RpcError::Remote {
                        command: command.clone(),
                        message: error.unwrap_or_else(|| "(no error message)".into()),
                    })
                };
                let _ = tx.send(result);
            } else {
                // V2.12.f · an unmatched response is usually the reply to a
                // fire-and-forget command (`prompt`, `steer`, `follow_up`).
                // Successful replies can be dropped, but FAILURES must be
                // surfaced — otherwise a user with no API credits sees
                // nothing happen. Forward errors as a synthetic event so
                // the UI can push an Entry::Error.
                if success {
                    tracing::debug!(id = %id, command = %command, "fire-and-forget response (ok)");
                } else {
                    let message = error.unwrap_or_else(|| "(no error message)".into());
                    tracing::warn!(
                        id = %id,
                        command = %command,
                        error = %message,
                        "unmatched error response — surfacing to UI"
                    );
                    let _ = evt_tx
                        .send(Incoming::CommandError {
                            command: command.clone(),
                            message,
                        })
                        .await;
                }
            }
        }
        other => {
            if evt_tx.send(other).await.is_err() {
                tracing::warn!("event channel closed; dropping event");
            }
        }
    }
}

async fn stderr_task(stderr: BufReader<ChildStderr>) {
    let mut lines = stderr.lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => tracing::warn!(target: "pi.stderr", "{line}"),
            Ok(None) => break,
            Err(e) => {
                tracing::error!(error = %e, "stderr read error");
                break;
            }
        }
    }
}

/// Gracefully shut down: send `abort`, signal writer to close stdin, wait on
/// the child with a short timeout, then kill.
pub async fn shutdown(client: RpcClient, mut io: RpcIo) -> Result<()> {
    let _ = client.fire(RpcCommand::Abort).await;
    client.shutdown().await;
    drop(client); // no new writes

    match tokio::time::timeout(std::time::Duration::from_millis(500), io.child.wait()).await {
        Ok(_) => {}
        Err(_) => {
            tracing::warn!("pi did not exit within 500ms; killing");
            io.child.kill().await.ok();
            io.child.wait().await.ok();
        }
    }

    let _ = io.tasks.writer.await;
    let _ = io.tasks.reader.await;
    let _ = io.tasks.stderr.await;
    Ok(())
}

/// Convenience: spawn pi and wrap it in a client in one call.
pub fn spawn(pi_bin: &str, pi_argv: &[String], debug_rpc: bool) -> Result<(RpcClient, RpcIo)> {
    let pi =
        super::process::spawn(pi_bin, pi_argv).wrap_err_with(|| eyre!("spawning pi {pi_bin:?}"))?;
    Ok(RpcClient::spawn(pi, debug_rpc))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an `RpcClient` that is wired to a local writer channel but has
    /// no reader/stderr tasks. Enough to exercise the send-path error and
    /// timeout semantics without a real pi process.
    fn test_client() -> (RpcClient, mpsc::Receiver<OutMsg>) {
        let (tx, rx) = mpsc::channel::<OutMsg>(16);
        let client = RpcClient {
            tx,
            pending: Arc::new(Mutex::new(HashMap::new())),
            id_counter: AtomicU64::new(1),
            debug_rpc: false,
        };
        (client, rx)
    }

    /// V3.a regression: if the writer channel is closed before `call` can
    /// send, the inserted pending entry must be removed — otherwise repeated
    /// Closed calls accumulate dead waiters in the map.
    #[tokio::test]
    async fn call_removes_pending_on_send_failure() {
        let (client, rx) = test_client();
        drop(rx); // writer side is dead
        let result = client.call(RpcCommand::GetState).await;
        assert!(matches!(result, Err(RpcError::Closed)));
        assert!(
            client.pending.lock().await.is_empty(),
            "pending map should be drained on send failure"
        );
    }

    /// V3.a: `call_timeout` surfaces `RpcError::Timeout(dur)` when pi never
    /// answers and cleans the pending map behind itself.
    #[tokio::test]
    async fn call_timeout_returns_timeout_when_idle() {
        let (client, _rx) = test_client(); // hold rx so send() succeeds
        let result = client
            .call_timeout(RpcCommand::GetState, Duration::from_millis(50))
            .await;
        assert!(
            matches!(result, Err(RpcError::Timeout(d)) if d == Duration::from_millis(50)),
            "expected Timeout(50ms), got {result:?}"
        );
        assert!(
            client.pending.lock().await.is_empty(),
            "pending map should be drained on timeout"
        );
    }
}
