use crate::err::{Error, Result};
use crate::fdio;
use crate::sel;
use crate::sys;
use crate::sys::fd_t;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_pipe::{PipeRead, PipeWrite};

async fn read_errors(mut read: PipeRead) -> Vec<String> {
    let mut errors = Vec::<String>::new();
    loop {
        let len = match read.read_u32_le().await {
            Ok(len) => len as usize,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => {
                errors.push(format!("error pipe: {}", e));
                break
            },
        };

        let mut buf = Vec::with_capacity(len);
        buf.resize(len, 0);
        if let Err(e) = read.read_exact(&mut buf[..]).await {
            errors.push(format!("error pipe: {}", e));
            break
        }

        errors.push(String::from_utf8_lossy(&buf).to_string());
    }

    return errors;
}

//------------------------------------------------------------------------------

pub struct ErrPipeRead {
    PipeRead: pipe,
    errs: Vec<String>,
}

impl ErrPipeRead {
    pub fn close(&self) -> Result<()> {
        sys::close(self.fd)?;
        Ok(())
    }

    pub fn get_errors(self) -> Vec<String> {
        self.errs
    }
}

impl sel::Read for ErrPipeRead {
    fn read(&mut self) -> bool {
        let err = match fdio::read_str(self.fd) {
            Ok(str) => str,
            Err(Error::Eof) => return true,
            Err(err) => panic!("error: {}", err),
        };
        self.errs.push(err);
        false
    }
}

pub struct ErrPipeWrite {
    fd: fd_t,
}

impl ErrPipeWrite {
    pub fn send(&self, err: &str) {
        fdio::write_str(self.fd, err).unwrap();
    }

    pub fn close(&self) -> Result<()> {
        sys::close(self.fd)?;
        Ok(())
    }
}

pub fn new_err_pipe() -> (ErrPipeRead, ErrPipeWrite) {
    let (read_fd, write_fd) = sys::pipe().unwrap_or_else(|err| {
        eprintln!("failed to create err pipe: {}", err);
        std::process::exit(exitcode::OSERR);
    });
    let err_read = ErrPipeRead {fd: read_fd, errs: Vec::new()};
    let err_write = ErrPipeWrite {fd: write_fd};
    (err_read, err_write)
}

