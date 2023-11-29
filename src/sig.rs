extern crate libc;

use lazy_static::lazy_static;
use libc::c_int;
use std::collections::HashMap;
use tokio::signal::unix::{signal, Signal, SignalKind};
use tokio::sync::watch::error::SendError;
use tokio::sync::watch::{channel, Receiver, Sender};

//------------------------------------------------------------------------------

pub type Signum = c_int;

// FIXME: `libc::NSIG` missing; would be better.
pub const NSIG: Signum = 32;

pub const SIGINT: Signum = 2;
pub const SIGQUIT: Signum = 3;
pub const SIGKILL: Signum = 9;
pub const SIGUSR1: Signum = 10;
pub const SIGTERM: Signum = 15;

#[rustfmt::skip]
lazy_static! {
    // FIXME: `libc::sigabbrev_np` missing; would be better.
    static ref SIGNAL_NUMS: HashMap::<&'static str, Signum> = {
        HashMap::from([
            ("SIGHUP"   ,  1),
            ("SIGINT"   , SIGINT),
            ("SIGQUIT"  , SIGQUIT),
            ("SIGILL"   ,  4),
            ("SIGTRAP"  ,  5),
            ("SIGABRT"  ,  6),
            ("SIGBUS"   ,  7),
            ("SIGFPE"   ,  8),
            ("SIGKILL"  , SIGKILL),
            ("SIGUSR1"  , SIGUSR1),
            ("SIGSEGV"  , 11),
            ("SIGUSR2"  , 12),
            ("SIGPIPE"  , 13),
            ("SIGALRM"  , 14),
            ("SIGTERM"  , SIGTERM),
            ("SIGCHLD"  , 17),
            ("SIGCONT"  , 18),
            ("SIGSTOP"  , 19),
            ("SIGTSTP"  , 20),
            ("SIGTTIN"  , 21),
            ("SIGTTOU"  , 22),
            ("SIGURG"   , 23),
            ("SIGXCPU"  , 24),
            ("SIGXFSZ"  , 25),
            ("SIGVTALRM", 26),
            ("SIGPROF"  , 27),
            ("SIGWINCH" , 28),
            ("SIGIO"    , 29),
            ("SIGPWR"   , 30),
            ("SIGSYS"   , 31),
        ])
    };

    static ref SIGNAL_ABBREVS: HashMap::<Signum, &'static str> = {
        SIGNAL_NUMS.iter().map(|(n, i)| (*i, *n)).collect::<HashMap<_, _>>()
    };
}

pub fn parse_signum(signum: &str) -> Option<Signum> {
    if let Some(signum) = SIGNAL_NUMS.get(signum) {
        Some(*signum)
    } else if let Ok(signum) = signum.parse::<c_int>() {
        Some(signum)
    } else {
        None
    }
}

pub fn get_abbrev(signum: Signum) -> Option<&'static str> {
    SIGNAL_ABBREVS.get(&signum).as_deref().copied()
}

//------------------------------------------------------------------------------

#[derive(Clone)]
pub struct SignalReceiver(Receiver<()>);

impl SignalReceiver {
    /// Blocks until a signal has been received.
    pub async fn signal(&mut self) {
        // changed() returns Error only if the sender is dropped, which it
        // shouldn't be because the signal watcher task runs in an infinite
        // loop.
        self.0.changed().await.unwrap();
    }
}

pub struct SignalWatcher {
    stream: Signal,
    sender: Sender<()>,
}

impl SignalWatcher {
    pub fn new(kind: SignalKind) -> (SignalWatcher, SignalReceiver) {
        let stream = signal(kind).unwrap();
        let (sender, receiver) = channel(());
        (SignalWatcher { stream, sender }, SignalReceiver(receiver))
    }

    pub async fn watch(mut self) {
        // Transmit all incoming signal events to the watch channel, until all
        // channel receivers are closed.
        loop {
            self.stream
                .recv()
                .await
                .expect("signal watcher stream recv");
            match self.sender.send(()) {
                Ok(()) => {}
                Err(SendError(())) => break,
            }
        }
    }
}
