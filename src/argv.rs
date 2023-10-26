use clap::Parser;

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

    /// process specification file
    pub input: Option<String>,

    /// write output to a file
    #[arg(short, long)]
    pub output: Option<String>,
}

pub fn parse() -> Args {
    Args::parse()
}
