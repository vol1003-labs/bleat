use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "bleat", about = "Durable messaging between coding agents")]
pub struct Cli {
    #[arg(long, global = true)]
    pub session: Option<String>,

    #[arg(long = "as", global = true)]
    pub role: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Init {
        slug: String,
    },
    Join,
    Send {
        #[arg(long)]
        to: String,
        #[arg(long = "type")]
        kind: Option<String>,
        #[arg(long = "re")]
        reply_to: Option<u64>,
        #[arg(long)]
        file: Option<PathBuf>,
        body: Option<String>,
    },
    Read {
        #[arg(long, conflicts_with = "wait")]
        peek: bool,
        #[arg(long, conflicts_with = "peek")]
        wait: bool,
        #[arg(long, requires = "wait")]
        timeout: Option<u64>,
    },
    Status,
    Log,
}
