mod metrics;
mod monitor;
mod output;

use clap::Parser;
use std::fs::File;
use std::io::{self, Write};
use std::process::{Command, ExitCode};

#[derive(Parser, Debug)]
#[command(name = "resstat", about = "Linux process resource sampler")]
struct Args {
    /// Sample/output interval in seconds (default: 1)
    #[arg(short = 's', long = "sample", default_value = "1")]
    sample: f64,

    /// Data refresh interval in seconds (default: 0.1)
    #[arg(short = 'r', long = "refresh", default_value = "0.1")]
    refresh: f64,

    /// Output file path (default: stdout)
    #[arg(short = 'o', long = "output")]
    output: Option<String>,

    /// Command and its arguments (after --)
    #[arg(last = true, required = true)]
    command: Vec<String>,
}

fn main() -> std::io::Result<ExitCode> {
    let args = Args::parse();

    // Validate arguments
    if args.refresh <= 0.0 {
        eprintln!("error: refresh interval must be > 0");
        return Ok(ExitCode::from(2));
    }
    if args.sample < args.refresh {
        eprintln!(
            "error: sample interval ({}) must be >= refresh interval ({})",
            args.sample, args.refresh
        );
        return Ok(ExitCode::from(2));
    }
    if args.command.is_empty() {
        eprintln!("error: no command specified after --");
        return Ok(ExitCode::from(2));
    }

    let cmd_name = &args.command[0];
    let cmd_args = &args.command[1..];

    // Spawn child process
    let child = match Command::new(cmd_name).args(cmd_args).spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: failed to spawn '{}': {}", cmd_name, e);
            return Ok(ExitCode::from(1));
        }
    };

    let pid = child.id();

    // Set up output writer
    let mut writer: Box<dyn Write> = match &args.output {
        Some(path) => match File::create(path) {
            Ok(f) => Box::new(f),
            Err(e) => {
                eprintln!("error: cannot open output file '{}': {}", path, e);
                return Ok(ExitCode::from(1));
            }
        },
        None => Box::new(io::stdout()),
    };

    let exit_code = monitor::run_monitor(child, args.sample, args.refresh, &mut writer, pid)?;
    Ok(ExitCode::from(exit_code as u8))
}
