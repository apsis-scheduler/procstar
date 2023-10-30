use clap::Parser;
use std::time::Duration;

use crate::wsclient;

//------------------------------------------------------------------------------

/// Run and manage processes.
#[derive(Parser, Debug)]
pub struct Args {
    /// run an HTTP service
    #[arg(short, long)]
    pub serve: bool,

    /// connect to a WebSocket server
    #[arg(short, long)]
    pub connect: Option<String>,
    /// identifying name for WebSocket connection
    #[arg(long)]
    pub name: Option<String>,
    /// group for WebSocket connection
    #[arg(long)]
    pub group_id: Option<String>,
    /// initial interval between connection attempts
    #[arg(long, value_name = "SECS")]
    pub connect_interval_start: Option<f64>,
    /// exponential backoff between connection attempts
    #[arg(long, value_name = "FAC")]
    pub connect_interval_mult: Option<f64>,
    /// maximum interval between connection attempts
    #[arg(long, value_name = "SECS")]
    pub connect_interval_max: Option<f64>,
    /// maximum number of connection attempts
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

pub fn get_connect_config(args: &Args) -> wsclient::ConnectConfig {
    eprintln!("argv: {:?}", args);
    // Defaults.
    let df = wsclient::ConnectConfig::new();
    // Apply options.
    wsclient::ConnectConfig {
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
