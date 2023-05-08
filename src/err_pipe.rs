use bytes::{BufMut, BytesMut};
use crate::err::Result;
use crate::fdio;
use crate::sys;
use std::io;
use std::os::fd::{AsRawFd, RawFd};
use tokio::io::AsyncReadExt;
use tokio_pipe::{PipeRead, PipeWrite};

/// Pipe for returning errors from the pre-exec child process to the parent.
/// This is to notify the parent of failures that might occur in operations that
/// have to be done in the child, such as setting up fds, and the exec call
/// itself.  The child writes zero or more short string messages, which the
/// parent accumulates.
///
/// 1. Create with [`ErrorPipe::new`] before forking.
///
/// 2. After forking in the child process, call [`in_child()`].  This returns
///    a writer for sending error messages back to the parent.  The post-fork
///    logic in the child process should not be async at all, and should not
///    return control to the reactor.
///
/// 3. After forking in the parent process, call [`in_parent()`].  This returns
///    a future for retrieving the accumulated error messages send from the
///    child.

pub struct ErrorPipe {
    read_pipe: PipeRead,
    write_pipe: PipeWrite,
}

pub struct ErrorWriter {
    write_pipe: PipeWrite,
}

impl ErrorWriter {
    pub fn write(&self, error: String) -> Result<()> {
        let bytes = error.as_bytes();
        // Encode the message len, which cannot be more than 64.
        assert!(bytes.len() <= u16::MAX as usize);
        let mut len = BytesMut::with_capacity(2);
        len.put_u16_le(bytes.len() as u16);

        // Write sync through the raw fd.
        let fd: RawFd = self.write_pipe.as_raw_fd();
        fdio::write(fd, &len)?;
        fdio::write(fd, &bytes)
    }
}

impl ErrorPipe {
    pub fn new() -> Result<ErrorPipe> {
        let (read_pipe, write_pipe) = tokio_pipe::pipe()?;
        Ok(ErrorPipe {
            read_pipe,
            write_pipe,
        })
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
        let ErrorPipe {
            read_pipe,
            write_pipe,
        } = self;
        // Drop and close the write pipe.
        std::mem::drop(write_pipe);
        ErrorPipe::get_errors(read_pipe).await
    }

    /// To be called immediatley after `fork()` in the child process.  This is
    /// intentionally not `async`; we do not ever return control to the reactor
    /// in the child process.
    pub fn in_child(self) -> io::Result<ErrorWriter> {
        let ErrorPipe {
            read_pipe,
            write_pipe,
        } = self;

        // Drop and close the read pipe.  We don't simply drop `read_pipe`
        // because of how Tokio registers fds for async notification.  If we
        // simply drop the read pipe, the read pipe in the parent doesn't wake
        // up the reactor anymore.  (I don't fully understand this.)
        sys::close(read_pipe.as_raw_fd()).unwrap();

        // Note that tokio-pipe sets CLOEXEC by default, so the write pipe will close
        // automatically in the child process when it exec's.
        Ok(ErrorWriter { write_pipe })
    }
}
