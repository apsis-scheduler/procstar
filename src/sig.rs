extern crate libc;

use lazy_static::lazy_static;
use libc::{c_int, sigset_t};
use std::collections::HashMap;
use std::io;
use tokio::signal::unix::{signal, Signal, SignalKind};
use tokio::sync::watch::error::SendError;
use tokio::sync::watch::{channel, Receiver, Sender};

//------------------------------------------------------------------------------

pub type Signum = c_int;

lazy_static! {
    static ref SIGNAL_NAMES: HashMap::<&'static str, Signum> = {
        HashMap::from([
            ("SIGHUP"   ,  1),
            ("SIGINT"   ,  2),
            ("SIGQUIT"  ,  3),
            ("SIGILL"   ,  4),
            ("SIGTRAP"  ,  5),
            ("SIGABRT"  ,  6),
            ("SIGBUS"   ,  7),
            ("SIGFPE"   ,  8),
            ("SIGKILL"  ,  9),
            ("SIGUSR1"  , 10),
            ("SIGSEGV"  , 11),
            ("SIGUSR2"  , 12),
            ("SIGPIPE"  , 13),
            ("SIGALRM"  , 14),
            ("SIGTERM"  , 15),
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
}

pub fn parse_signum(signum: &str) -> Option<Signum> {
    if let Some(signum) = SIGNAL_NAMES.get(signum) {
        Some(*signum)
    } else if let Ok(signum) = signum.parse::<c_int>() {
        Some(signum)
    } else {
        None
    }
}

//------------------------------------------------------------------------------

/// Signal handler callback fn.
// FIXME: ucontext_t?
// FIXME: Handler type has to be predicated on flags & SA_SIGINFO.  With
//        SA_SIGINFO, the signature is,
//            extern "system" fn(c_int, *const libc::siginfo_t, *const libc::c_void);
type Sighandler = extern "system" fn(c_int) -> ();

pub fn empty_sigset() -> sigset_t {
    unsafe { std::mem::zeroed() }
}

#[cfg(not(target_os = "linux"))]
fn make_sigaction(
    sa_sigaction: libc::sighandler_t,
    sa_mask: sigset_t,
    sa_flags: c_int,
) -> libc::sigaction {
    libc::sigaction {
        sa_sigaction,
        sa_mask,
        sa_flags,
    }
}

#[cfg(target_os = "linux")]
fn make_sigaction(
    sa_sigaction: libc::sighandler_t,
    sa_mask: sigset_t,
    sa_flags: c_int,
) -> libc::sigaction {
    libc::sigaction {
        sa_sigaction,
        sa_mask,
        sa_flags,
        sa_restorer: None, // Linux only
    }
}

fn empty_sigaction() -> libc::sigaction {
    make_sigaction(libc::SIG_DFL, empty_sigset(), 0)
}

impl std::convert::Into<libc::sigaction> for Sigaction {
    fn into(self) -> libc::sigaction {
        make_sigaction(
            match self.disposition {
                Sigdisposition::Default => libc::SIG_DFL,
                Sigdisposition::Ignore => libc::SIG_IGN,
                Sigdisposition::Handler(h) => h as libc::sighandler_t,
            },
            self.mask,
            self.flags,
        )
    }
}

impl std::convert::From<libc::sigaction> for Sigaction {
    fn from(sa: libc::sigaction) -> Self {
        Self {
            disposition: match sa.sa_sigaction {
                libc::SIG_DFL => Sigdisposition::Default,
                libc::SIG_IGN => Sigdisposition::Ignore,
                h => Sigdisposition::Handler(unsafe {
                    std::mem::transmute::<libc::sighandler_t, Sighandler>(h)
                }),
            },
            mask: sa.sa_mask,
            flags: sa.sa_flags,
        }
    }
}

pub enum Sigdisposition {
    Default,
    Ignore,
    Handler(Sighandler),
}

pub struct Sigaction {
    pub disposition: Sigdisposition,
    pub mask: sigset_t,
    pub flags: c_int,
}

/// Sets and/or retrieves the signal action of `signum`.
///
/// If sigaction is `Some`, sets the signal action and returns the previous.  If
/// sigaction is `None`, retrieves the signal action without changing it.
pub fn sigaction(signum: c_int, sigaction: Option<Sigaction>) -> io::Result<Sigaction> {
    let act: libc::sigaction;
    let act_ptr = match sigaction {
        Some(sa) => {
            act = sa.into();
            &act
        }
        None => std::ptr::null(),
    };
    let mut old = empty_sigaction();
    match unsafe { libc::sigaction(signum, act_ptr, &mut old) } {
        -1 => Err(io::Error::last_os_error()),
        0 => Ok(Sigaction::from(old)),
        ret => panic!("sigaction returned {}", ret),
    }
}

//------------------------------------------------------------------------------

// FIXME: NSIG is not reliably available in libc.  I hope this is enough.
const NSIG: usize = 256;

pub struct SignalFlag {
    signum: usize,
}

static mut SIGNAL_FLAGS: [bool; NSIG] = [false; NSIG];

/// Hacky unsafe boolean flag for a signal.  Installs a signal handler that sets
/// the flag when the signal is received.
impl SignalFlag {
    pub fn new(signum: c_int) -> Self {
        assert!(signum > 0);
        assert!(signum < NSIG as c_int);

        extern "system" fn handler(signum: c_int) {
            // Accessing a static global is in general not threadsafe, but this
            // signal handler will only ever be called on the main thread.
            unsafe {
                SIGNAL_FLAGS[signum as usize] = true;
            }
        }

        // Set up the handler.
        // FIXME: Check that we're not colliding with an existing handler.
        sigaction(
            signum,
            Some(Sigaction {
                disposition: Sigdisposition::Handler(handler),
                mask: empty_sigset(),
                flags: libc::SA_NOCLDSTOP,
            }),
        )
        .unwrap_or_else(|err| {
            eprintln!("sigaction failed: {}", err);
            std::process::exit(exitcode::OSERR);
        });

        Self {
            signum: signum as usize,
        }
    }

    /// Retrieves the flag value, and clears it.
    pub fn get(&self) -> bool {
        unsafe {
            let val = SIGNAL_FLAGS[self.signum];
            SIGNAL_FLAGS[self.signum] = false;
            val
        }
    }
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
