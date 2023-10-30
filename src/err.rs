use crate::proto;

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
    Eof,
    Io(std::io::Error),
    NativeTlsError(native_tls::Error),
    ParseInt(std::num::ParseIntError),
    Proto(proto::Error),
    Websocket(tokio_tungstenite::tungstenite::error::Error),
}

impl Error {
    pub fn last_os_error() -> Error {
        Error::Io(std::io::Error::last_os_error())
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match *self {
            Error::Eof => f.write_str("EOF"),
            Error::Io(ref err) => err.fmt(f),
            Error::NativeTlsError(ref err) => err.fmt(f),
            Error::ParseInt(ref err) => err.fmt(f),
            Error::Proto(ref err) => err.fmt(f),
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

impl From<tokio_tungstenite::tungstenite::Error> for Error {
    fn from(err: tokio_tungstenite::tungstenite::Error) -> Error {
        Error::Websocket(err)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
