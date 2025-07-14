use std::os::fd::RawFd;

use crate::proto;
use crate::shutdown;
use crate::spec;
use crate::spec::ProcId;

//------------------------------------------------------------------------------

/// An error in the specification of a process.
#[derive(Debug)]
pub enum SpecError {
    DupId(Vec<String>),
}

impl std::fmt::Display for SpecError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            SpecError::DupId(ids) => {
                f.write_str("duplicate proc IDs: ")?;
                f.write_str(&ids.join(" "))
            }
        }
    }
}

//------------------------------------------------------------------------------

/// All the potentially user-visible things that can go wrong while setting up
/// or running a process.
#[derive(Debug)]
pub enum Error {
    /// Tombstone.
    None,
    /// Premature EOF; this is a protocol error.
    Eof,
    /// Wraps an I/O error.
    Io(std::io::Error),
    /// Wraps a JSON parsing error.
    Json(serde_json::Error),
    /// Wraps a TLS connection error.
    NativeTlsError(native_tls::Error),
    /// Unknown file descriptor.
    NoFd(RawFd),
    /// Underlying OS process is missing.
    NoProc,
    /// Unknown process ID.
    NoProcId(ProcId),
    /// Wraps an integer parsing error.
    ParseInt(std::num::ParseIntError),
    /// Process in wrong state: must be running, but is not.
    ProcNotRunning(ProcId),
    /// Process in wrong state: must not be running, but is.
    ProcRunning(ProcId),
    /// Protocol error.
    Proto(proto::Error),
    RegisterTimeout,
    /// Wraps a RMP (MessagePack) decoding error.
    RMPDecode(rmp_serde::decode::Error),
    /// Wraps a RMP (MessagePack) encoding error.
    RMPEncode(rmp_serde::encode::Error),
    /// An agent is shutting down.
    ShuttingDown(shutdown::State),
    /// Wraps a proc spec error.
    Spec(spec::Error),
    /// Wraps a systemd zbus error.
    Systemd(zbus::Error),
    /// Wraps a WebSocket connection error.
    Websocket(tokio_tungstenite::tungstenite::error::Error),
}

impl Error {
    pub fn last_os_error() -> Error {
        Error::Io(std::io::Error::last_os_error())
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::None => f.write_str("(no error)"),
            Error::Eof => f.write_str("EOF"),
            Error::Io(ref err) => err.fmt(f),
            Error::Json(ref err) => err.fmt(f),
            Error::NativeTlsError(ref err) => err.fmt(f),
            Error::NoFd(fd) => write!(f, "no fd: {fd}"),
            Error::NoProc => write!(f, "no process"),
            Error::NoProcId(proc_id) => write!(f, "unknown proc ID: {proc_id}"),
            Error::ParseInt(ref err) => err.fmt(f),
            Error::ProcNotRunning(proc_id) => write!(f, "process not running: {proc_id}"),
            Error::ProcRunning(proc_id) => write!(f, "process running: {proc_id}"),
            Error::Proto(ref err) => err.fmt(f),
            Error::RegisterTimeout => write!(f, "registration took too long"),
            Error::RMPDecode(ref err) => err.fmt(f),
            Error::RMPEncode(ref err) => err.fmt(f),
            Error::ShuttingDown(shutdown_state) => {
                write!(f, "agent shutting down: {shutdown_state}")
            }
            Error::Spec(ref err) => err.fmt(f),
            Error::Systemd(ref err) => write!(f, "error with systemd: {err}"),
            Error::Websocket(ref err) => err.fmt(f),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Error {
        Error::Io(err)
    }
}

impl From<native_tls::Error> for Error {
    fn from(err: native_tls::Error) -> Error {
        Error::NativeTlsError(err)
    }
}

impl From<std::num::ParseIntError> for Error {
    fn from(err: std::num::ParseIntError) -> Error {
        Error::ParseInt(err)
    }
}

impl From<proto::Error> for Error {
    fn from(err: proto::Error) -> Error {
        Error::Proto(err)
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Error {
        Error::Json(err)
    }
}

impl From<rmp_serde::decode::Error> for Error {
    fn from(err: rmp_serde::decode::Error) -> Error {
        Error::RMPDecode(err)
    }
}

impl From<rmp_serde::encode::Error> for Error {
    fn from(err: rmp_serde::encode::Error) -> Error {
        Error::RMPEncode(err)
    }
}

impl From<spec::Error> for Error {
    fn from(err: spec::Error) -> Error {
        Error::Spec(err)
    }
}

impl From<zbus::Error> for Error {
    fn from(err: zbus::Error) -> Error {
        Error::Systemd(err)
    }
}

impl From<tokio_tungstenite::tungstenite::Error> for Error {
    fn from(err: tokio_tungstenite::tungstenite::Error) -> Error {
        Error::Websocket(err)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
