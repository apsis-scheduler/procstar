/// Named "Res" to avoid confusion with the `Result` types.

use base64::Engine;
use crate::spec::CaptureFormat;
use libc::{c_int, pid_t, rusage};
use std::collections::BTreeMap;
use std::path::PathBuf;
use serde::{Serialize};

//------------------------------------------------------------------------------

/// Mirror types to derive Serialize for libc remote types.
/// https://serde.rs/remote-derive.html
mod libc_serde {

    use libc::{c_long, suseconds_t, time_t};
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize)]
    #[serde(remote = "libc::timeval")]
    pub struct Timeval {
        pub tv_sec: time_t,
        pub tv_usec: suseconds_t,
    }

    #[derive(Serialize, Deserialize)]
    #[serde(remote = "libc::rusage")]
    pub struct Rusage {
        #[serde(with = "Timeval")]
        pub ru_utime: libc::timeval,
        #[serde(with = "Timeval")]
        pub ru_stime: libc::timeval,
        pub ru_maxrss: c_long,
        #[cfg(all(target_arch = "x86_64", target_pointer_width = "32"))]
        __pad1: u32,
        pub ru_ixrss: c_long,
        #[cfg(all(target_arch = "x86_64", target_pointer_width = "32"))]
        __pad2: u32,
        pub ru_idrss: c_long,
        #[cfg(all(target_arch = "x86_64", target_pointer_width = "32"))]
        __pad3: u32,
        pub ru_isrss: c_long,
        #[cfg(all(target_arch = "x86_64", target_pointer_width = "32"))]
        __pad4: u32,
        pub ru_minflt: c_long,
        #[cfg(all(target_arch = "x86_64", target_pointer_width = "32"))]
        __pad5: u32,
        pub ru_majflt: c_long,
        #[cfg(all(target_arch = "x86_64", target_pointer_width = "32"))]
        __pad6: u32,
        pub ru_nswap: c_long,
        #[cfg(all(target_arch = "x86_64", target_pointer_width = "32"))]
        __pad7: u32,
        pub ru_inblock: c_long,
        #[cfg(all(target_arch = "x86_64", target_pointer_width = "32"))]
        __pad8: u32,
        pub ru_oublock: c_long,
        #[cfg(all(target_arch = "x86_64", target_pointer_width = "32"))]
        __pad9: u32,
        pub ru_msgsnd: c_long,
        #[cfg(all(target_arch = "x86_64", target_pointer_width = "32"))]
        __pad10: u32,
        pub ru_msgrcv: c_long,
        #[cfg(all(target_arch = "x86_64", target_pointer_width = "32"))]
        __pad11: u32,
        pub ru_nsignals: c_long,
        #[cfg(all(target_arch = "x86_64", target_pointer_width = "32"))]
        __pad12: u32,
        pub ru_nvcsw: c_long,
        #[cfg(all(target_arch = "x86_64", target_pointer_width = "32"))]
        __pad13: u32,
        pub ru_nivcsw: c_long,
        #[cfg(all(target_arch = "x86_64", target_pointer_width = "32"))]
        __pad14: u32,

        #[cfg(any(target_env = "musl", target_os = "emscripten"))]
        __reserved: [c_long; 16],
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

#[derive(Serialize)]
pub struct ProcRes {
    /// Errors starting the process.
    pub errors: Vec<String>,
    /// The pid with which the process ran.
    pub pid: pid_t,

    /// Pid status, which combines exit code and signum.
    pub status: c_int,
    /// Exit code (low 8 bits), if terminated with exit.
    pub exit_code: Option<i32>,
    /// Signal number, if terminated by signal.
    pub signum: Option<i32>,
    /// Whether the process produced a core dump, if terminated by signal.
    pub core_dump: bool,

    /// Fd results.
    /// FIXME: Associative map from fd instead?
    pub fds: BTreeMap<String, FdRes>,

    /// Resource usage for the process itself.
    #[serde(with = "libc_serde::Rusage")]
    pub rusage: rusage,
}

fn time_to_sec(time: libc::timeval) -> f64 {
    time.tv_sec as f64 + 1e-6 * time.tv_usec as f64
}

impl ProcRes {
    pub fn new(errors: Vec<String>, pid: pid_t, status: c_int, rusage: rusage) -> ProcRes {
        let (exit_code, signum, core_dump)= {
            if libc::WIFEXITED(status) {
                (Some(libc::WEXITSTATUS(status)), None, false)
            } else {
                (None, Some(libc::WTERMSIG(status)), libc::WCOREDUMP(status))
            }
        };
        ProcRes {
            errors,
            pid,
            status,
            exit_code, signum, core_dump,
            fds: BTreeMap::new(),
            rusage,
        }
    }

    /// User time in s.
    pub fn utime(&self) -> f64 {
        time_to_sec(self.rusage.ru_utime)
    }

    /// System time in s.
    pub fn stime(&self) -> f64 {
        time_to_sec(self.rusage.ru_stime)
    }
}

//------------------------------------------------------------------------------

#[derive(Default, Serialize)]
pub struct Res {
    pub procs: Vec<ProcRes>,
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

