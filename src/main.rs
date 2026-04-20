mod anim;
mod app;
mod cli;
mod clipboard;
mod history;
mod log;
mod rpc;
mod term_caps;
mod theme;
mod ui;

use clap::Parser;
use color_eyre::eyre::Result;

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = cli::Args::parse();
    let _log = log::init(args.log_level.as_deref())?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    runtime.block_on(app::run(args))
}
