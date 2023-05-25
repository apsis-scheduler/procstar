use clap::Parser;

//------------------------------------------------------------------------------

/// Run and manage processes.
#[derive(Parser, Debug)]
pub struct Args {
    /// run an HTTP service
    #[arg(short, long)]
    pub serve: bool,

    pub input: Option<String>,
}

pub fn parse() -> Args {
    Args::parse()
}
