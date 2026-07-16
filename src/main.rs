use std::io::{self, Write};
use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    let cli = bleat::cli::Cli::parse();
    match bleat::app::run(cli) {
        Ok(output) => {
            let mut stdout = io::stdout().lock();
            let mut stderr = io::stderr().lock();
            if stdout.write_all(output.stdout.as_bytes()).is_err()
                || stderr.write_all(output.stderr.as_bytes()).is_err()
            {
                return ExitCode::from(1);
            }
            ExitCode::from(output.exit_code)
        }
        Err(error) => {
            let _ = writeln!(io::stderr().lock(), "{error}");
            ExitCode::from(error.exit_code())
        }
    }
}
