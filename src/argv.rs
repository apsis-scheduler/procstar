use clap::Parser;

//------------------------------------------------------------------------------

/// Run and manage processes.
#[derive(Parser, Debug)]
pub struct Args {
    /// run an HTTP service
    #[arg(short, long)]
    pub serve: bool,

    /// process specification file
    pub input: Option<String>,

    /// write output to a file
    #[arg(short, long)]
    pub output: Option<String>,
}

pub fn parse() -> Args {
    Args::parse()
}
