use std::cell::RefCell;
use std::os::fd::RawFd;
use std::path::PathBuf;
use std::rc::Rc;
use tokio::io::AsyncReadExt;
use tokio::task::JoinHandle;
use tokio_pipe::PipeRead;

use crate::err::Result;
use crate::res::FdRes;
use crate::spec;
use crate::sys;

//------------------------------------------------------------------------------

pub fn parse_fd(fd: &str) -> std::result::Result<RawFd, std::num::ParseIntError> {
    match fd {
        "stdin" => Ok(0),
        "stdout" => Ok(1),
        "stderr" => Ok(2),
        _ => fd.parse::<RawFd>(),
    }
}

pub fn get_fd_name(fd: RawFd) -> String {
    match fd {
        0 => "stdin".to_string(),
        1 => "stdout".to_string(),
        2 => "stderr".to_string(),
        _ => fd.to_string(),
    }
}

//------------------------------------------------------------------------------

// FIXME: Generalize: split out R/W/RW from file creation flags.
fn get_oflags(flags: &spec::OpenFlag, fd: RawFd) -> libc::c_int {
    use spec::OpenFlag::*;
    match flags {
        Default => match fd {
            0 => libc::O_RDONLY,
            1 | 2 => libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC,
            _ => libc::O_RDWR | libc::O_CREAT | libc::O_TRUNC,
        },
        Read => libc::O_RDONLY,
        Write => libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC,
        Create => libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        Replace => libc::O_WRONLY | libc::O_TRUNC,
        Append => libc::O_WRONLY | libc::O_APPEND,
        CreateAppend => libc::O_WRONLY | libc::O_CREAT | libc::O_APPEND,
        ReadWrite => libc::O_RDWR | libc::O_CREAT | libc::O_TRUNC,
    }
}

//------------------------------------------------------------------------------

#[derive(Debug)]
pub enum FdHandler {
    Inherit,

    /// Closes the file descriptor.
    Close {
        /// Proc-visible fd.
        fd: RawFd,
    },

    /// Dup's the file descriptor to an open file.  That file is not managed.
    UnmanagedFile {
        /// Proc-visible fd.
        fd: RawFd,
        /// Fd open to the file.
        file_fd: RawFd,
    },

    /// Attaches the file descriptor to a pipe; reads data from the pipe and
    /// buffers in memory.
    CaptureMemory {
        /// Proc-visible fd.
        fd: RawFd,
        /// Read end of the pipe.
        read_fd: RawFd,
        /// Write end of the pipe.
        write_fd: RawFd,
        /// Format for output.
        format: spec::CaptureFormat,
        /// Captured output.
        buf: Vec<u8>,
    },
}

pub struct SharedFdHandler(Rc<RefCell<FdHandler>>);

//------------------------------------------------------------------------------

const PATH_DEV_NULL: &str = "/dev/null";

impl SharedFdHandler {
    pub fn new(fd: RawFd, spec: spec::Fd) -> Result<Self> {
        let fd = match spec {
            spec::Fd::Inherit => FdHandler::Inherit,

            spec::Fd::Close => FdHandler::Close { fd },

            spec::Fd::Null { flags } => {
                let path = PathBuf::from(PATH_DEV_NULL);
                let oflags = get_oflags(&flags, fd);
                let file_fd = sys::open(&path, oflags, 0)?;
                FdHandler::UnmanagedFile { fd, file_fd }
            }

            spec::Fd::File { path, flags, mode } => {
                let path = PathBuf::from(path);
                let oflags = get_oflags(&flags, fd);
                let file_fd = sys::open(&path, oflags, mode)?;
                FdHandler::UnmanagedFile { fd, file_fd }
            }

            spec::Fd::Capture {
                mode: spec::CaptureMode::Memory,
                format,
            } => {
                let (read_fd, write_fd) = sys::pipe()?;
                FdHandler::CaptureMemory {
                    fd,
                    read_fd,
                    write_fd,
                    format,
                    buf: Vec::new(),
                }
            }

            _ => panic!("missing"),
        };
        Ok(SharedFdHandler(Rc::new(RefCell::new(fd))))
    }

    /// Reads from the pipe read fd, appending data to `buf`, until EOF.
    async fn run_capture_memory(rc: Rc<RefCell<FdHandler>>) -> Result<()> {
        let mut read_pipe = if let FdHandler::CaptureMemory { read_fd, .. } = *rc.borrow() {
            PipeRead::from_raw_fd_checked(read_fd)?
        } else {
            panic!();
        };

        let mut read_buf = [0 as u8; 65536]; // FIXME: What's the right size?
        loop {
            // Don't hold a ref to the handler object across await, so `buf` is
            // accessible elsewhere.
            let len = read_pipe.read(&mut read_buf).await?;
            if len == 0 {
                break;
            } else {
                if let &mut FdHandler::CaptureMemory { ref mut buf, .. } = &mut *rc.borrow_mut() {
                    buf.extend_from_slice(&read_buf[..len]);
                } else {
                    panic!();
                };
            }
        }
        Ok(())
    }

    pub fn in_parent(&self) -> Result<Option<JoinHandle<Result<()>>>> {
        Ok(match *self.0.borrow() {
            FdHandler::Inherit => None,

            FdHandler::Close { .. } => None,

            FdHandler::UnmanagedFile { file_fd, .. } => {
                sys::close(file_fd).unwrap();
                None
            }

            FdHandler::CaptureMemory { write_fd, .. } => {
                // In the parent, we only read.
                sys::close(write_fd)?;
                // Start a task to drain the pipe into the buffer.
                Some(tokio::task::spawn_local(Self::run_capture_memory(
                    Rc::clone(&self.0),
                )))
            }
        })
    }

    pub fn in_child(self) -> Result<()> {
        match Rc::try_unwrap(self.0).unwrap().into_inner() {
            FdHandler::Inherit => Ok(()),

            FdHandler::Close { fd } => {
                sys::close(fd)?;
                Ok(())
            }

            FdHandler::UnmanagedFile { fd, file_fd } => {
                if file_fd != fd {
                    sys::dup2(file_fd, fd)?;
                    sys::close(file_fd)?;
                }
                Ok(())
            }

            FdHandler::CaptureMemory {
                fd,
                read_fd,
                write_fd,
                ..
            } => {
                // In the child, don't read from the pipe.
                sys::close(read_fd)?;
                // Attach the write pipe to the target fd.
                sys::dup2(write_fd, fd)?;
                Ok(())
            }
        }
    }

    pub fn get_result(&self) -> Result<FdRes> {
        // FIXME: Should we provide more information here?
        Ok(match &*self.0.borrow() {
            FdHandler::Inherit => FdRes::None,

            FdHandler::Close { .. } => FdRes::None,

            FdHandler::UnmanagedFile { .. } => FdRes::None,

            FdHandler::CaptureMemory { format, buf, .. } => FdRes::from_bytes(*format, buf),
        })
    }
}
