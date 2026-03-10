mod cli;
mod client;
mod db;
mod logging;
mod models;
mod server;
#[cfg(feature = "tui")]
mod tui;

use clap::Parser;

pub fn version() -> String {
    flo::version()
}

#[tokio::main]
async fn main() {
    let cli = cli::Cli::parse();
    if let Err(e) = cli::run(cli).await {
        eprintln!("error: {:#}", e);
        std::process::exit(1);
    }
}
