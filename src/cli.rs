use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "rata-pi",
    version,
    about = "Ratatui TUI for the Pi coding agent (RPC)"
)]
pub struct Args {
    /// Path to the pi binary (default: "pi" on PATH).
    #[arg(long, default_value = "pi")]
    pub pi_bin: String,

    /// LLM provider (anthropic, openai, google, ...). Passed through to pi.
    #[arg(long)]
    pub provider: Option<String>,

    /// Model pattern or ID. Passed through to pi.
    #[arg(long)]
    pub model: Option<String>,

    /// Custom session storage directory. Passed through to pi.
    #[arg(long)]
    pub session_dir: Option<String>,

    /// Disable pi session persistence.
    #[arg(long)]
    pub no_session: bool,

    /// Log every RPC line to the log file (verbose).
    #[arg(long)]
    pub debug_rpc: bool,

    /// Override log level (trace|debug|info|warn|error). Also honors RUST_LOG.
    #[arg(long)]
    pub log_level: Option<String>,
}

impl Args {
    /// Build the argv passed to `pi --mode rpc ...`.
    pub fn pi_argv(&self) -> Vec<String> {
        let mut v = vec!["--mode".into(), "rpc".into()];
        if let Some(p) = &self.provider {
            v.push("--provider".into());
            v.push(p.clone());
        }
        if let Some(m) = &self.model {
            v.push("--model".into());
            v.push(m.clone());
        }
        if let Some(d) = &self.session_dir {
            v.push("--session-dir".into());
            v.push(d.clone());
        }
        if self.no_session {
            v.push("--no-session".into());
        }
        v
    }
}
