use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::config::ApprovalMode;

/// AI-powered security CLI.
#[derive(Parser, Debug)]
#[command(name = "seval", about = "AI-powered security CLI", version)]
pub struct Cli {
    /// Subcommand to run.
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// AWS profile name.
    #[arg(long)]
    pub profile: Option<String>,

    /// AWS region.
    #[arg(long)]
    pub region: Option<String>,

    /// Bedrock model ID.
    #[arg(long)]
    pub model: Option<String>,

    /// Tool approval mode.
    #[arg(long, value_enum)]
    pub approval_mode: Option<ApprovalMode>,

    /// Path to configuration file.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Run non-interactively: send a prompt, stream output to stdout, then exit.
    #[arg(short = 'p', long = "pipe")]
    pub pipe: Option<String>,
}

/// Available subcommands.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Initialize configuration (interactive wizard).
    Init {
        /// Overwrite existing config.
        #[arg(long)]
        force: bool,
    },
}
