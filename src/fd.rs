use libc::c_int;
use log::*;
use std::cell::RefCell;
use std::fs;
use std::io::{Read, Seek};
use std::os::fd::{FromRawFd, IntoRawFd, RawFd};
use std::path::PathBuf;
use std::rc::Rc;
use tokio::io::AsyncReadExt;
use tokio::task::JoinHandle;
use tokio_pipe::PipeRead;

use crate::err::{Error, Result};
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
    /// Inherits the existing file descriptor, if any.
    Inherit,

    /// A failed attempt to create a file descriptor.
    Error {
        /// Error message.
        err: Error,
    },

    /// Closes the file descriptor.
    Close {
        /// Proc-visible fd.
        fd: RawFd,
    },

    /// Dup's the file descriptor from another.
    Dup {
        /// Proc-visible fd.
        fd: RawFd,
        /// Fd to be dup'ed.
        dup_fd: RawFd,
    },

    /// Dup's the file descriptor to an open file.  That file is not managed.
    UnmanagedFile {
        /// Proc-visible fd.
        fd: RawFd,
        path: PathBuf,
        oflags: i32,
        mode: c_int,
    },

    UnlinkedFile {
        /// Proc-visible fd.
        fd: RawFd,
        /// Fd open to the file.
        file_fd: RawFd,
        /// Format for output.
        format: spec::CaptureFormat,
        /// Whether to attach output to results.
        attached: bool,
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
        /// Whether to attach output to results.
        attached: bool,
    },
}

impl FdHandler {
    /// Creates an unmanaged file fd handler.  The file itself is opened in the
    /// child process.
    fn new_unmanaged_file(fd: RawFd, path: &str, flags: spec::OpenFlag, mode: c_int) -> FdHandler {
        let path = PathBuf::from(path);
        let oflags = get_oflags(&flags, fd);
        FdHandler::UnmanagedFile {
            fd,
            path,
            oflags,
            mode,
        }
    }
}

pub struct SharedFdHandler(Rc<RefCell<FdHandler>>);

//------------------------------------------------------------------------------

const PATH_DEV_NULL: &str = "/dev/null";
// FIXME: Correct tmpdir.
const PATH_TMP_TEMPLATE: &str = "/tmp/ir-capture-XXXXXXXXXXXX";

/// Creates and opens an unlinked temporary file as a fd handler.
fn open_unlinked_temp_file(
    fd: RawFd,
    format: spec::CaptureFormat,
    attached: bool,
) -> Result<FdHandler> {
    // Open a temp file.
    let (tmp_path, tmp_fd) = sys::mkstemp(PATH_TMP_TEMPLATE)?;
    // Unlink it.
    std::fs::remove_file(tmp_path)?;
    // Since it's unlinked, we don't have to deal with it later.
    Ok(FdHandler::UnlinkedFile {
        fd,
        file_fd: tmp_fd,
        format,
        attached,
    })
}

/// Reads the contents of a file from the beginning, from its open fd.
fn read_file_from_start(fd: RawFd) -> Result<Vec<u8>> {
    // Wrap the fd in a file object, for convenience.  This takes ownership of the fd.
    let mut file = unsafe { fs::File::from_raw_fd(fd) };
    // Seek to front.
    file.rewind()?;
    // Read the contents.
    let mut buf = Vec::<u8>::new();
    file.read_to_end(&mut buf)?;
    // Take back ownership of the fd.
    assert!(file.into_raw_fd() == fd);
    Ok(buf)
}

impl SharedFdHandler {
    pub fn new(fd: RawFd, spec: spec::Fd) -> Result<Self> {
        let fd_handler = match spec {
            spec::Fd::Inherit => FdHandler::Inherit,

            spec::Fd::Close => FdHandler::Close { fd },

            spec::Fd::Null { flags } => FdHandler::new_unmanaged_file(fd, PATH_DEV_NULL, flags, 0),

            spec::Fd::Dup { fd: dup_fd } => FdHandler::Dup { fd, dup_fd },

            spec::Fd::File { path, flags, mode } => {
                FdHandler::new_unmanaged_file(fd, &path, flags, mode)
            }

            spec::Fd::Capture {
                mode: spec::CaptureMode::TempFile,
                format,
                attached,
                ..
            } => open_unlinked_temp_file(fd, format, attached)?,

            spec::Fd::Capture {
                mode: spec::CaptureMode::Memory,
                format,
                attached,
            } => {
                let (read_fd, write_fd) = sys::pipe()?;
                FdHandler::CaptureMemory {
                    fd,
                    read_fd,
                    write_fd,
                    format,
                    buf: Vec::new(),
                    attached,
                }
            }
        };
        Ok(SharedFdHandler(Rc::new(RefCell::new(fd_handler))))
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

            FdHandler::Error { .. } => None,

            FdHandler::Close { .. } => None,

            FdHandler::Dup { .. } => None,

            FdHandler::UnmanagedFile { .. } => None,

            FdHandler::UnlinkedFile { .. } => None,

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

            FdHandler::Error { err } => Err(err),

            FdHandler::Close { fd } => {
                sys::close(fd)?;
                Ok(())
            }

            FdHandler::Dup { fd, dup_fd } => {
                if dup_fd != fd {
                    sys::dup2(dup_fd, fd)?;
                }
                Ok(())
            }

            FdHandler::UnmanagedFile {
                fd,
                path,
                oflags,
                mode,
            } => {
                let file_fd = sys::open(&path, oflags, mode)?;
                if file_fd != fd {
                    sys::dup2(file_fd, fd)?;
                    sys::close(file_fd)?;
                }
                Ok(())
            }

            FdHandler::UnlinkedFile { fd, file_fd, .. } => {
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
            FdHandler::Inherit
            | FdHandler::Error { .. }
            | FdHandler::Close { .. }
            | FdHandler::Dup { .. }
            | FdHandler::UnmanagedFile { .. } => FdRes::None,

            FdHandler::UnlinkedFile {
                file_fd,
                format,
                attached,
                ..
            } => {
                if *attached {
                    FdRes::from_bytes(*format, &read_file_from_start(*file_fd)?)
                } else {
                    FdRes::Detached
                }
            }

            FdHandler::CaptureMemory {
                format,
                buf,
                attached,
                ..
            } => {
                if *attached {
                    FdRes::from_bytes(*format, buf)
                } else {
                    FdRes::Detached
                }
            }
        })
    }
}

//------------------------------------------------------------------------------

pub fn make_fd_handler(fd_str: String, spec: spec::Fd) -> (RawFd, SharedFdHandler) {
    // FIXME: Parse, or at least check, when deserializing.
    let fd_num = parse_fd(&fd_str).unwrap_or_else(|err| {
        error!("failed to parse fd {}: {}", fd_str, err);
        std::process::exit(2);
    });

    let handler = SharedFdHandler::new(fd_num, spec)
        .unwrap_or_else(|err| SharedFdHandler(Rc::new(RefCell::new(FdHandler::Error { err }))));

    (fd_num, handler)
}
