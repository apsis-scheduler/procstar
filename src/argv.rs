use clap::Parser;
use std::time::Duration;

use procstar::agent;
use procstar::proto::DEFAULT_PORT;

//------------------------------------------------------------------------------

pub fn parse_log_level(level: &str) -> Result<log::Level, ()> {
    match level {
        "trace" => Ok(log::Level::Trace),
        "debug" => Ok(log::Level::Debug),
        "info" => Ok(log::Level::Info),
        "warn" => Ok(log::Level::Warn),
        "error" => Ok(log::Level::Error),
        _ => Err(()),
    }
}

/// Run and manage processes.
#[derive(Parser, Debug)]
#[command(about)]
pub struct Args {
    /// Log at LEVEL
    #[arg(
        long, value_name = "LEVEL",
        value_parser = clap::builder::PossibleValuesParser::new(["trace", "debug", "info", "warn", "error"])
    )]
    pub log_level: Option<String>,

    /// Only start processes with executable at PATH
    #[arg(long, value_name = "PATH")]
    pub restrict_exe: Option<String>,

    /// When no processes are running, print results and exit
    #[arg(short, long)]
    pub print: bool,
    /// When no processes are running, write results to PATH and exit
    #[arg(short, long, value_name = "PATH")]
    pub output: Option<String>,
    /// When no processes are running, exit
    #[arg(short = 'x', long)]
    pub exit: bool,

    /// When idle (all processes deleted), exit
    #[arg(short = 'w', long)]
    pub wait: bool,

    /// Run an HTTP service
    #[arg(short, long)]
    pub serve: bool,
    /// Serve HTTP on PORT
    #[arg(long, value_name = "PORT", default_value_t = 3000)]
    pub serve_port: u16,

    /// Connect to a server as an agent
    #[arg(short, long)]
    pub agent: bool,
    /// Connect as agent to HOST
    #[arg(long, value_name = "HOST")]
    pub agent_host: Option<String>,
    /// Connect as agent to PORT
    #[arg(long, value_name = "PORT", default_value_t = DEFAULT_PORT)]
    pub agent_port: u32,
    /// Connection ID for agent connection; for debugging only
    #[arg(long, hide = true, value_name = "ID")]
    pub conn_id: Option<String>,
    /// Agent group ID
    #[arg(long)]
    pub group_id: Option<String>,
    /// Initial interval between agent connection attempts
    #[arg(long, value_name = "SECS")]
    pub connect_interval_start: Option<f64>,
    /// Exponential backoff between agent connection attempts
    #[arg(long, value_name = "FAC")]
    pub connect_interval_mult: Option<f64>,
    /// Maximum interval between agent connection attempts
    #[arg(long, value_name = "SECS")]
    pub connect_interval_max: Option<f64>,
    /// Maximum number of consecutive agent connection attempts
    #[arg(long, value_name = "COUNT")]
    pub connect_count_max: Option<u64>,
    /// Agent websocket read timeout in seconds; for testing only
    #[arg(long, hide = true, value_name = "SECS")]
    pub agent_read_timeout: Option<f64>,

    /// Process specification file, or "-" for stdin
    pub input: Option<String>,
}

pub fn parse() -> Args {
    let mut args = Args::parse();

    if args.print || args.output.is_some() {
        args.exit = true;
    }

    if args.exit && args.wait {
        eprintln!("Specify only one of --exit or --wait.");
    }

    if !(args.exit || args.wait || args.serve || args.agent) {
        eprintln!("Usage: Specify at least one of --exit, --wait, --serve, or --agent.");
        std::process::exit(exitcode::DATAERR);
    }

    args
}

pub fn get_connect_config(args: &Args) -> agent::ConnectConfig {
    // Defaults.
    let df = agent::ConnectConfig::new();
    // Apply options.
    agent::ConnectConfig {
        interval_start: args
            .connect_interval_start
            .map_or(df.interval_start, Duration::from_secs_f64),
        interval_mult: args.connect_interval_mult.unwrap_or(df.interval_mult),
        interval_max: args
            .connect_interval_max
            .map_or(df.interval_max, Duration::from_secs_f64),
        count_max: args.connect_count_max.unwrap_or(df.count_max),
        read_timeout: args
            .agent_read_timeout
            .map_or(df.read_timeout, Duration::from_secs_f64),
    }
}
