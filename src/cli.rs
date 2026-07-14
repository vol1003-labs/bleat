use std::ffi::OsString;
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
    Spawn {
        role: String,
        #[arg(last = true, required = true)]
        argv: Vec<OsString>,
    },
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
    Nudge {
        role: String,
    },
    Read {
        #[arg(long)]
        peek: bool,
    },
    Status,
    Log,
}
