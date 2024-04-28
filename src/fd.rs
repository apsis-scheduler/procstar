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
use crate::spec::parse_fd;
use crate::sys;

//------------------------------------------------------------------------------

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

pub struct FdData {
    pub data: Vec<u8>,
    pub encoding: Option<spec::CaptureEncoding>,
}

impl FdData {
    pub fn empty() -> Self {
        Self { data: Vec::new(), encoding: None, }
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
        encoding: Option<spec::CaptureEncoding>,
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
        encoding: Option<spec::CaptureEncoding>,
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
    encoding: Option<spec::CaptureEncoding>,
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
        encoding,
        attached,
    })
}

/// Reads the contents of a file from the beginning, from its open fd.
fn read_from_file(fd: RawFd, start: u64, stop: Option<u64>) -> Result<Vec<u8>> {
    // Wrap the fd in a file object, for convenience.  This takes ownership of the fd.
    let mut file = unsafe { fs::File::from_raw_fd(fd) };
    // Seek to front.
    file.seek(std::io::SeekFrom::Start(start))?;
    let buf = if let Some(stop) = stop {
        // Read to indicated stop position.
        if stop < start {
            return Err(Error::Eof);
        }
        let mut buf = vec![0; (stop - start) as usize];
        file.read_exact(&mut buf)?;
        buf
    } else {
        // Read entire contents.
        let mut buf = Vec::<u8>::new();
        file.read_to_end(&mut buf)?;
        buf
    };

    // Take back ownership of the fd.
    assert!(file.into_raw_fd() == fd);
    Ok(buf)
}

fn get_file_length(fd: RawFd) -> Result<i64> {
    Ok(sys::fstat(fd)?.st_size)
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
                encoding,
                attached,
                ..
            } => open_unlinked_temp_file(fd, encoding, attached)?,

            spec::Fd::Capture {
                mode: spec::CaptureMode::Memory,
                encoding,
                attached,
            } => {
                let (read_fd, write_fd) = sys::pipe()?;
                FdHandler::CaptureMemory {
                    fd,
                    read_fd,
                    write_fd,
                    encoding,
                    buf: Vec::new(),
                    attached,
                }
            },

            spec::Fd::PipeWrite => panic!(),
            spec::Fd::PipeRead { proc_id, fd } => panic!(),
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

    /// Returns data for the fd, if available, and whether it is UTF-8 text.
    pub fn get_data(&self, start: usize, stop: Option<usize>) -> Result<Option<FdData>> {
        Ok(match &*self.0.borrow() {
            FdHandler::Inherit
            | FdHandler::Error { .. }
            | FdHandler::Close { .. }
            | FdHandler::Dup { .. }
            | FdHandler::UnmanagedFile { .. } => None,

            FdHandler::UnlinkedFile {
                file_fd,
                encoding,
                ..
            } => Some(FdData {
                data: read_from_file(*file_fd, start as u64, stop.map(|s| s as u64))?,
                encoding: *encoding,
            }),

            FdHandler::CaptureMemory {
                buf,
                encoding,
                ..
            } => {
                let stop = stop.unwrap_or_else(|| buf.len());
                Some(FdData {
                    data: buf[start..stop].to_vec(),
                    encoding: *encoding,
                })
            }
        })
    }

    pub fn get_result(&self) -> Result<Option<FdRes>> {
        // FIXME: Should we provide more information here?
        Ok(match &*self.0.borrow() {
            FdHandler::Inherit
            | FdHandler::Error { .. }
            | FdHandler::Close { .. }
            | FdHandler::Dup { .. }
            // FIXME: Return something containing the path.
            | FdHandler::UnmanagedFile { .. } => None,

            FdHandler::UnlinkedFile {
                file_fd,
                encoding,
                attached,
                ..
            } => {
                Some(if *attached {
                    FdRes::from_bytes(*encoding, &read_from_file(*file_fd, 0, None)?)
                } else {
                    FdRes::detached(get_file_length(*file_fd)?, *encoding)
                })
            }

            FdHandler::CaptureMemory {
                encoding,
                buf,
                attached,
                ..
            } => {
                Some(if *attached {
                    FdRes::from_bytes(*encoding, buf)
                } else {
                    FdRes::detached(buf.len() as i64, *encoding)
                })
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
