use clap::Parser;

use seval::cli::{Cli, Commands};
use seval::config::AppConfig;
use seval::errors::install_panic_hook;
use seval::logging::init_logging;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    install_panic_hook();

    let _logging_guard = init_logging()?;

    let cli = Cli::parse();

    tracing::info!("seval starting, version {}", env!("CARGO_PKG_VERSION"));

    match cli.command {
        Some(Commands::Init { force }) => {
            if !force && AppConfig::has_global_config() {
                let path = seval::config::global_config_path()?;
                eprintln!(
                    "Config already exists at {}. Use --force to overwrite.",
                    path.display()
                );
                return Ok(());
            }
            let mut app = seval::app::App::new_wizard_mode()?;
            app.run().await?;
        }
        None => {
            if !AppConfig::has_global_config() {
                // First run -- launch wizard.
                let mut app = seval::app::App::new_wizard_mode()?;
                app.run().await?;
            }
            // Now proceed to normal app (config should exist).
            let config = AppConfig::load()?;
            let mut app = seval::app::App::new(&config).await?;
            app.run().await?;
        }
    }

    Ok(())
}
