/// Named "Res" to avoid confusion with the `Result` types.

use base64::Engine;
use crate::spec::{ProcId, CaptureFormat};
use libc::{c_int, pid_t, rusage};
use std::collections::BTreeMap;
use std::path::PathBuf;
use serde::Serialize;

//------------------------------------------------------------------------------

/// Analogue to libc's `struct rusage`.  We use our own struct,
///
/// - to omit fields not used by Linux
/// - to convert some values to more convenient units
/// - because `libc::rusage` doesn't implement `serde::Serialize`.
///
#[derive(Debug, Serialize)]
pub struct ResourceUsage {
    /// User CPU time used.
    pub utime: f64,
    /// System CPU time used.
    pub stime: f64,
    /// Maximum resident set size.
    pub maxrss: u64,
    /// Page reclaims (minor / soft page faults).
    pub minflt: u64,
    /// Page faults (major / hard page faults).
    pub majflt: u64,
    /// Swaps.
    pub nswap: u64,
    /// Block input operations.
    pub inblock: u64,
    /// Block output operations.
    pub oublock: u64,
    /// Voluntary context switches.
    pub nvcsw: u64,
    /// Involuntary context switches.
    pub nivcsw: u64,
}

fn time_to_sec(time: libc::timeval) -> f64 {
    time.tv_sec as f64 + 1e-6 * time.tv_usec as f64
}

impl ResourceUsage {
    pub fn new(r: &rusage) -> Self {
        Self {
            utime: time_to_sec(r.ru_utime),
            stime: time_to_sec(r.ru_stime),
            maxrss: (r.ru_maxrss as u64) * 1024,  // convert KiB to bytes
            minflt: r.ru_minflt as u64,
            majflt: r.ru_majflt as u64,
            nswap: r.ru_nswap as u64,
            inblock: r.ru_inblock as u64,
            oublock: r.ru_oublock as u64,
            nvcsw: r.ru_nvcsw as u64,
            nivcsw: r.ru_nivcsw as u64,
        }
    }
}

//------------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all="lowercase")]
#[serde(untagged)]
pub enum FdRes {
    Error,

    None {
    },

    File {
        path: PathBuf,
    },

    CaptureUtf8 {
        text: String,
    },

    CaptureBase64 {
        data: String,
        encoding: String,
    },
}

impl FdRes {
    pub fn from_bytes(format: CaptureFormat, buffer: Vec<u8>) -> FdRes {
        match format {
            CaptureFormat::Text => {
                // FIXME: Handle errors.
                let text = String::from_utf8_lossy(&buffer).to_string();
                FdRes::CaptureUtf8 {
                    text
                }
            },
            CaptureFormat::Base64 => {
                // FIXME: Handle errors.
                let data = base64::engine::general_purpose::STANDARD_NO_PAD.encode(
                    &buffer, 
                );
                FdRes::CaptureBase64 {
                    data,
                    encoding: "base64".to_string()
                }
            },
        }
    }
}

//------------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct Status {
    /// The raw process exit status returned by `wait()`.  This combines exit
    /// code and signum.
    pub status: c_int,
    /// Process exit code, if terminated with exit.
    pub exit_code: Option<i32>,
    /// Signal number, if terminated by signal.
    pub signum: Option<i32>,
    /// True if the process was terminated by a signal and produced a core dump.
    pub core_dump: bool,
}

impl Status {
    pub fn new(status: c_int) -> Self {
        let (exit_code, signum, core_dump)= {
            if libc::WIFEXITED(status) {
                (Some(libc::WEXITSTATUS(status)), None, false)
            } else {
                (None, Some(libc::WTERMSIG(status)), libc::WCOREDUMP(status))
            }
        };
        Self { status, exit_code, signum, core_dump }
    }
}

#[derive(Serialize)]
pub struct ProcRes {
    /// Errors starting the process.
    pub errors: Vec<String>,
    /// The pid with which the process ran.
    pub pid: pid_t,

    /// Process status, if it has completed.
    pub status: Option<Status>,

    /// Process resource usage, if it has completed.
    pub rusage: Option<ResourceUsage>,

    /// Fd results.
    /// FIXME: Associative map from fd instead?
    pub fds: BTreeMap<String, FdRes>,
}

impl ProcRes {
    pub fn new(errors: Vec<String>, pid: pid_t, status: c_int, rusage: rusage) -> Self {
        Self {
            errors,
            pid,
            status: Some(Status::new(status)),
            rusage: Some(ResourceUsage::new(&rusage)),
            fds: BTreeMap::new(),
        }
    }
}

//------------------------------------------------------------------------------

#[derive(Default, Serialize)]
pub struct Res {
    pub procs: BTreeMap<ProcId, ProcRes>,
    pub errors: Vec<String>,
}

impl Res {
    pub fn new() -> Res {
        Res { ..Default::default() }
    }
}

//------------------------------------------------------------------------------

pub fn print(result: &Res) {
    serde_json::to_writer(std::io::stdout(), result).unwrap();
}

