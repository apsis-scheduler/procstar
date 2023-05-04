/// All the potentially user-visible things that can go wrong while setting up
/// or running a process.

//------------------------------------------------------------------------------

#[derive(Debug)]
pub enum Error {
    Eof,
    Io(std::io::Error),
    ParseInt(std::num::ParseIntError),
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
            Error::ParseInt(ref err) => err.fmt(f),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Error {
        Error::Io(err)
    }
}

impl From<std::num::ParseIntError> for Error {
    fn from(err: std::num::ParseIntError) -> Error {
        Error::ParseInt(err)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

