use bytes::{BufMut, BytesMut};
use std::io;
use std::os::fd::RawFd;
use tokio::io::AsyncReadExt;
use tokio_pipe::PipeRead;

use crate::err::Result;
use crate::fdio;
use crate::sys;

//------------------------------------------------------------------------------

/// Pipe for returning errors from the pre-exec child process to the parent.
/// This is to notify the parent of failures that might occur in operations that
/// have to be done in the child, such as setting up fds, and the exec call
/// itself.  The child writes zero or more short string messages, which the
/// parent accumulates.
///
/// 1. Create with [`ErrorPipe::new`] before forking.
///
/// 2. After forking in the child process, call [`in_child()`].  This returns
///    a writer for (syncronous) sending error messages back to the parent.
///
/// 3. After forking in the parent process, call [`in_parent()`].  This returns
///    an async future for retrieving the accumulated error messages send from
///    the child.

pub struct ErrorPipe {
    read_fd: RawFd,
    write_fd: RawFd,
}

pub struct ErrorWriter {
    write_fd: RawFd,
}

impl ErrorWriter {
    pub fn write(&self, error: String) -> Result<()> {
        let bytes = error.as_bytes();
        // Encode the message len, which cannot be more than 64.
        assert!(bytes.len() <= u16::MAX as usize);
        let mut len = BytesMut::with_capacity(2);
        len.put_u16_le(bytes.len() as u16);

        // Write sync through the raw fd.
        fdio::write(self.write_fd, &len)?;
        fdio::write(self.write_fd, &bytes)
    }

    /// Like [`write`], but ignores errors.
    pub fn try_write(&self, error: String) {
        _ = self.write(error);
    }
}

impl ErrorPipe {
    pub fn new() -> Result<ErrorPipe> {
        let (read_fd, write_fd) = sys::pipe()?;
        sys::set_cloexec(read_fd)?;
        sys::set_cloexec(write_fd)?;
        Ok(ErrorPipe { read_fd, write_fd })
    }

    async fn get_errors(mut read_pipe: PipeRead) -> Vec<String> {
        let mut errors = Vec::new();
        loop {
            // Read the message length.
            let q = read_pipe.read_u16_le().await;
            let len = match q {
                Ok(len) =>
                    // Got the length.
                    len as usize,
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof
                    // EOF means the other end is done.
                    => break,
                Err(e) => {
                    // Any other error is a problem.  There isn't a good place to
                    // report it, so add it as an error here.
                    errors.push(format!("error pipe: {}", e));
                    break
                },
            };

            // We know exactly how long the message should be.
            let mut buf = Vec::with_capacity(len);
            buf.resize(len, 0);
            if let Err(e) = read_pipe.read_exact(&mut buf[..]).await {
                // Unable to read the message.  There isn't a good place to report
                // the error, so report it here.
                errors.push(format!("error pipe: {}", e));
                break;
            }

            // Accumulate this error.
            errors.push(String::from_utf8_lossy(&buf).to_string());
        }

        errors
    }

    pub async fn in_parent(self) -> Vec<String> {
        let ErrorPipe { read_fd, write_fd } = self;

        // Close the write pipe.  This is necessary so that we see EOF on the
        // read pipe once the child process closes the other open read fd,
        // usually when it exec's.
        sys::close(write_fd).unwrap();

        // Wrap the read pipe for async.
        let read_pipe = PipeRead::from_raw_fd_checked(read_fd).unwrap();
        ErrorPipe::get_errors(read_pipe).await
    }

    /// To be called immediatley after `fork()` in the child process.  This is
    /// intentionally not `async`; we do not ever return control to the reactor
    /// in the child process.
    pub fn in_child(self) -> io::Result<ErrorWriter> {
        let ErrorPipe { read_fd, write_fd } = self;

        // Close the read pipe.
        sys::close(read_fd).unwrap();

        // Since the pipe is CLOEXEC, so the write pipe will close automatically
        // in the child process when it exec's.
        Ok(ErrorWriter { write_fd })
    }
}
