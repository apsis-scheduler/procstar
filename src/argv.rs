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
pub struct Args {
    /// log at LEVEL
    #[arg(
        long, value_name = "LEVEL",
        value_parser = clap::builder::PossibleValuesParser::new(["trace", "debug", "info", "warn", "error"])
    )]
    pub log_level: Option<String>,

    /// run an HTTP service
    #[arg(short, long)]
    pub serve: bool,

    /// connect to a server as an agent
    #[arg(short, long)]
    pub agent: bool,
    /// connect as agent to HOST
    #[arg(long, value_name = "HOST")]
    pub agent_host: Option<String>,
    /// connect as agent to PORT
    #[arg(long, value_name = "PORT", default_value_t = DEFAULT_PORT)]
    pub agent_port: u32,
    /// connection ID for agent connection; for debugging only
    #[arg(long, hide = true, value_name = "ID")]
    pub conn_id: Option<String>,
    /// agent group ID
    #[arg(long)]
    pub group_id: Option<String>,
    /// initial interval between agent connection attempts
    #[arg(long, value_name = "SECS")]
    pub connect_interval_start: Option<f64>,
    /// exponential backoff between agent connection attempts
    #[arg(long, value_name = "FAC")]
    pub connect_interval_mult: Option<f64>,
    /// maximum interval between agent connection attempts
    #[arg(long, value_name = "SECS")]
    pub connect_interval_max: Option<f64>,
    /// maximum number of consecutive agent connection attempts
    #[arg(long, value_name = "COUNT")]
    pub connect_count_max: Option<u64>,

    /// process specification file
    pub input: Option<String>,

    /// write output to a file
    #[arg(short, long)]
    pub output: Option<String>,
}

pub fn parse() -> Args {
    Args::parse()
}

pub fn get_connect_config(args: &Args) -> agent::ConnectConfig {
    // Defaults.
    let df = agent::ConnectConfig::new();
    // Apply options.
    agent::ConnectConfig {
        interval_start: args
            .connect_interval_start
            .map_or(df.interval_start, |s| Duration::from_secs_f64(s)),
        interval_mult: args.connect_interval_mult.unwrap_or(df.interval_mult),
        interval_max: args
            .connect_interval_max
            .map_or(df.interval_max, |s| Duration::from_secs_f64(s)),
        count_max: args.connect_count_max.unwrap_or(df.count_max),
    }
}
