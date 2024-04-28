use libc::c_int;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::fs::File;
use std::io::BufReader;
use std::os::fd::RawFd;
use std::path::Path;
use std::string::String;
use std::vec::Vec;

use crate::sys::fd_t;

//------------------------------------------------------------------------------
// Spec error
//------------------------------------------------------------------------------

#[derive(Debug)]
pub enum Error {
    BadFd(String),
    DuplicateFd(FdName),
    DuplicateProcId(ProcId),
    Io(std::io::Error),
    Json(serde_json::error::Error),
    UnmatchedReadFd(ProcId, FdName),
    UnmatchedWriteFd(ProcId, FdName),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::BadFd(name) => f.write_str(&format!("bad fd: {}", name)),
            Error::DuplicateFd(fd) => f.write_str(&format!("duplicate fd: {}", fd)),
            Error::Io(err) => err.fmt(f),
            Error::Json(err) => err.fmt(f),
            Error::DuplicateProcId(proc_id) => {
                f.write_str(&format!("duplicate proc id: {}", proc_id))
            }
            Error::UnmatchedReadFd(proc_id, fd) => {
                f.write_str(&format!("unmatched read pipe: {} {}", proc_id, fd))
            }
            Error::UnmatchedWriteFd(proc_id, fd) => {
                f.write_str(&format!("unmatched write pipe: {} {}", proc_id, fd))
            }
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Error {
        Error::Io(err)
    }
}

impl From<serde_json::error::Error> for Error {
    fn from(err: serde_json::error::Error) -> Error {
        Error::Json(err)
    }
}

type Result<T> = std::result::Result<T, Error>;

//------------------------------------------------------------------------------
// Env spec
//------------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub enum EnvInherit {
    None,
    All,
    Vars(Vec<String>),
}

impl Default for EnvInherit {
    fn default() -> Self {
        Self::All
    }
}

impl<'de> Deserialize<'de> for EnvInherit {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = EnvInherit;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("true, false, or seq of env var names")
            }

            fn visit_bool<E>(self, v: bool) -> std::result::Result<Self::Value, E> {
                Ok(if v {
                    Self::Value::All
                } else {
                    Self::Value::None
                })
            }

            fn visit_seq<S>(self, mut seq: S) -> std::result::Result<Self::Value, S::Error>
            where
                S: serde::de::SeqAccess<'de>,
            {
                let mut vars = Vec::new();
                while let Some(var) = seq.next_element()? {
                    vars.push(var);
                }
                Ok(Self::Value::Vars(vars))
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct Env {
    pub inherit: EnvInherit,
    pub vars: BTreeMap<String, Option<String>>,
}

//------------------------------------------------------------------------------
// Fd spec
//------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum OpenFlag {
    // FIXME: Generalize.
    /// Equivalent to `Read` for stdin, `Write` for stdout/stderr,
    /// `ReadWrite` for others.
    Default,

    /// Open existing file for reading.
    Read,
    /// Create or open exsting file for writing.
    Write,
    /// Create a new file for writing; file may not exist.
    Create,
    /// Overwrite an existing file for writing; file must exist.
    Replace,
    /// Create or open an existing file for appending.
    CreateAppend,
    /// Open an existing file for appending.
    Append,
    /// Create or open existing file for reading and writing.
    ReadWrite,
}

impl Default for OpenFlag {
    fn default() -> Self {
        Self::Default
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "lowercase")]
pub enum CaptureMode {
    TempFile,
    Memory,
}

impl Default for CaptureMode {
    fn default() -> Self {
        Self::TempFile
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "lowercase")]
pub enum CaptureEncoding {
    #[serde(rename = "utf-8")]
    Utf8,
}

impl Default for CaptureEncoding {
    fn default() -> Self {
        Self::Utf8
    }
}

fn get_default_mode() -> c_int {
    0o666
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "lowercase")]
pub enum Fd {
    /// Inherit this fd from the parent process, if any.
    Inherit,

    /// Close this fd, if it's open.
    Close,

    /// Open this fd to /dev/null.
    Null {
        #[serde(default)]
        flags: OpenFlag,
    },

    /// Open this fd to a file.
    File {
        path: String,
        #[serde(default)]
        flags: OpenFlag,
        #[serde(default = "get_default_mode")]
        mode: c_int,
        // format
    },

    /// Duplicate another existing fd to this one.
    Dup { fd: fd_t },

    /// Capture output from fd; include in results.
    Capture {
        #[serde(default)]
        mode: CaptureMode,

        #[serde(default)]
        encoding: Option<CaptureEncoding>,

        #[serde(default = "Fd::attached_default")]
        attached: bool,
    },

    /// Write from this process to another through a pipe.
    #[serde(rename = "pipe-write")]
    PipeWrite,

    /// Read from this process from another through a pipe.
    #[serde(rename = "pipe-read")]
    PipeRead {
        /// The proc whose pipe from which to read.
        proc_id: ProcId,
        /// The fd of the proc writing to the pipe, from which to read.
        fd: FdName,
    },
}

impl Fd {
    fn attached_default() -> bool {
        true
    }
}

impl Default for Fd {
    fn default() -> Self {
        Self::Inherit
    }
}

pub fn parse_fd(fd: &str) -> std::result::Result<RawFd, Error> {
    match fd {
        "stdin" => Ok(0),
        "stdout" => Ok(1),
        "stderr" => Ok(2),
        _ => fd.parse::<RawFd>().map_err(|_| Error::BadFd(fd.to_owned())),
    }
}

/// Set of proc fds that are connected to the ends of pipes.
pub type PipeFds = HashSet<(ProcId, RawFd)>;

/// Validates fd descriptors in procs.
///
/// Checks for:
/// - a fd specified more than once for one proc
/// - an unmatched read pipe or write pipe
///
/// Returns the set of (proc id, raw fd) that are connected to the write ends of
/// pipes.
pub fn validate_procs_fds(procs: &Procs) -> std::result::Result<(), Error> {
    // Check for duplicate fds.
    for (_proc_id, proc) in procs.iter() {
        let mut fds = HashSet::<RawFd>::new();
        for (fd_name, _) in proc.fds.iter() {
            if !fds.insert(parse_fd(fd_name)?) {
                return Err(Error::DuplicateFd(fd_name.clone()));
            }
        }
    }

    // Collect write pipes.
    let mut unmatched_pipes = PipeFds::new();
    for (proc_id, proc) in procs.iter() {
        for (fd_name, fd) in proc.fds.iter() {
            match fd {
                Fd::PipeWrite => {
                    let key = (proc_id.clone(), parse_fd(fd_name)?);
                    assert!(unmatched_pipes.insert(key));
                }
                _ => (),
            }
        }
    }
    // Match up read pipes.
    for (_proc_id, proc) in procs.iter() {
        for (_, fd) in proc.fds.iter() {
            match fd {
                Fd::PipeRead {
                    proc_id: from_proc_id,
                    fd: from_fd_name,
                } => {
                    let key = (from_proc_id.clone(), parse_fd(from_fd_name)?);
                    if !unmatched_pipes.remove(&key) {
                        // A read pipe with no matching write pipe.
                        return Err(Error::UnmatchedReadFd(key.0, from_fd_name.clone()));
                    };
                }
                _ => (),
            }
        }
    }
    // Remaining write pipes are unmatched.
    for (proc_id, fd_num) in unmatched_pipes.into_iter() {
        return Err(Error::UnmatchedWriteFd(proc_id.clone(), fd_num.to_string()));
    }

    Ok(())
}

//------------------------------------------------------------------------------
// Process spec
//------------------------------------------------------------------------------

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(default)]
pub struct Proc {
    pub id: Option<String>,
    pub exe: Option<String>,
    pub argv: Vec<String>,
    pub env: Env,
    pub fds: Vec<(FdName, Fd)>,
}

pub type Procs = BTreeMap<ProcId, Proc>;

//------------------------------------------------------------------------------
// Input spec
//------------------------------------------------------------------------------

pub type ProcId = String;
pub type FdName = String;

#[derive(Serialize, Deserialize, Default, Debug)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub specs: Procs,
}

impl Input {
    pub fn new() -> Self {
        Self {
            specs: Procs::new(),
        }
    }
}

pub fn load_stdin() -> Result<Input> {
    let stdin = std::io::stdin().lock();
    let spec = serde_json::from_reader(stdin)?;
    Ok(spec)
}

pub fn load_file<P: AsRef<Path>>(path: P) -> Result<Input> {
    // Open the file in read-only mode with buffer.
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    // Read the JSON contents of the file.
    let spec = serde_json::from_reader(reader)?;

    // Return the spec.
    Ok(spec)
}
