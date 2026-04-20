use std::{fs, path::PathBuf};

use color_eyre::eyre::Result;
use directories::ProjectDirs;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

/// Hold this for the lifetime of the program — drops the non-blocking writer's worker.
pub struct LogGuard {
    _guard: WorkerGuard,
    #[allow(dead_code)] // exposed for future UI use (e.g. `?` shortcut to show log path)
    pub log_dir: PathBuf,
}

/// Initialize tracing to a rolling daily log file.
///
/// Stdout and stderr are reserved for the RPC pipe and the TTY — we never write
/// to them from tracing. Level precedence: `RUST_LOG` > `--log-level` > "info".
pub fn init(cli_level: Option<&str>) -> Result<LogGuard> {
    let log_dir = resolve_log_dir();
    fs::create_dir_all(&log_dir)?;

    let file_appender = tracing_appender::rolling::daily(&log_dir, "rata-pi.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let env = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(cli_level.unwrap_or("info")))
        .unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(env)
        .with(
            fmt::layer()
                .with_ansi(false)
                .with_writer(non_blocking)
                .with_target(true),
        )
        .init();

    tracing::info!(log_dir = ?log_dir, "tracing initialized");
    Ok(LogGuard {
        _guard: guard,
        log_dir,
    })
}

fn resolve_log_dir() -> PathBuf {
    if let Some(dirs) = ProjectDirs::from("dev", "olivvein", "rata-pi") {
        if let Some(state) = dirs.state_dir() {
            return state.to_path_buf();
        }
        return dirs.data_local_dir().to_path_buf();
    }
    std::env::temp_dir().join("rata-pi")
}
