//! Spawn `pi --mode rpc ...` and expose its stdio handles.
//!
//! stderr is captured (kept piped) so pi errors don't bleed onto the TTY and we can
//! surface them in the UI. `kill_on_drop(true)` guarantees we never leak the child if
//! the app panics before graceful shutdown.

// stdin/stdout/stderr readers become the RPC I/O loop in M1.
#![allow(dead_code)]

use std::process::Stdio;

use color_eyre::eyre::{Context, Result, eyre};
use tokio::io::BufReader;
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};

pub struct PiProcess {
    pub child: Child,
    pub stdin: ChildStdin,
    pub stdout: BufReader<ChildStdout>,
    pub stderr: BufReader<ChildStderr>,
}

pub fn spawn(pi_bin: &str, argv: &[String]) -> Result<PiProcess> {
    let mut cmd = Command::new(pi_bin);
    cmd.args(argv)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = cmd.spawn().with_context(|| {
        format!(
            "failed to spawn pi binary {pi_bin:?}. \
             Is pi installed and on PATH? \
             Install with `npm i -g @mariozechner/pi-coding-agent` or pass --pi-bin."
        )
    })?;

    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| eyre!("no stdin on child"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| eyre!("no stdout on child"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| eyre!("no stderr on child"))?;

    Ok(PiProcess {
        child,
        stdin,
        stdout: BufReader::new(stdout),
        stderr: BufReader::new(stderr),
    })
}
