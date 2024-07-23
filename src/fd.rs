use libc::c_int;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::os::fd::RawFd;
use std::path::PathBuf;
use std::rc::Rc;
use tokio::io::AsyncReadExt;
use tokio::task::JoinHandle;
use tokio_pipe::PipeRead;

use crate::err::{Error, Result};
use crate::res::FdRes;
use crate::spec;
use crate::spec::{parse_fd, ProcId};
use crate::sys;
use crate::sys::RWPair;

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
        Self {
            data: Vec::new(),
            encoding: None,
        }
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

    /// Attaches the file descriptor to the write end of a pipe; the read end is
    /// attached to another process.
    PipeWrite { fd: RawFd, pipe_write_fd: RawFd },

    /// Attaches the file descriptor to the read end of a pipe; the write end is
    /// attached to another process.
    PipeRead { fd: RawFd, pipe_read_fd: RawFd },
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

//------------------------------------------------------------------------------

const PATH_DEV_NULL: &str = "/dev/null";
// FIXME: Correct tmpdir.
const PATH_TMP_TEMPLATE: &str = "/tmp/procstar-capture-XXXXXXXXXXXX";

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

//------------------------------------------------------------------------------

pub struct Pipes {
    fds: BTreeMap<(ProcId, RawFd), RWPair<Option<RawFd>>>,
}

/// Manages pipes connecting fds in procs.
///
/// We may set up the proc that connects to either the read or the write end
/// first.  If the read end, we return the read fd and save the write fd, else
/// vice versa.
impl Pipes {
    pub fn new() -> Self {
        Self {
            fds: BTreeMap::new(),
        }
    }

    /// Returns the read fd, if it exists, for reading the output from `from_fd`
    /// of `from_proc_id`.  If not, creates a new pipe, returns the read fd, and
    /// stores the write fd for later.
    pub fn get_read_fd(&mut self, from_proc_id: &ProcId, from_fd: RawFd) -> Result<RawFd> {
        let key = (from_proc_id.clone(), from_fd);
        match self.fds.get_mut(&key) {
            Some(RWPair { read, .. }) => Ok(read.take().unwrap()),
            None => {
                let pipe = sys::pipe()?;
                self.fds.insert(
                    key,
                    RWPair {
                        read: None,
                        write: Some(pipe.write),
                    },
                );
                Ok(pipe.read)
            }
        }
    }

    /// Returns the write fd, if it exists, for writing the output from `fd` of
    /// `proc_id`.  If not, creates a new pipe, returns the write fd, and saves
    /// the read fd for later.
    pub fn get_write_fd(&mut self, proc_id: &ProcId, fd: RawFd) -> Result<RawFd> {
        let key = (proc_id.clone(), fd);
        match self.fds.get_mut(&key) {
            Some(RWPair { write, .. }) => Ok(write.take().unwrap()),
            None => {
                let pipe = sys::pipe()?;
                self.fds.insert(
                    key,
                    RWPair {
                        read: Some(pipe.read),
                        write: None,
                    },
                );
                Ok(pipe.write)
            }
        }
    }

    /// Closes all remaining pipe fds.  Returns the number of fds closed.
    pub fn close(self) -> Result<usize> {
        let mut count = 0;
        for (_, pipe) in self.fds.into_iter() {
            if let Some(fd) = pipe.read {
                sys::close(fd)?;
                count += 1;
            }
            if let Some(fd) = pipe.write {
                sys::close(fd)?;
                count += 1;
            }
        }
        Ok(count)
    }
}

//------------------------------------------------------------------------------

pub struct SharedFdHandler(Rc<RefCell<FdHandler>>);

/// Implements the handling of a file descriptor for a proc.
///
/// The lifecycle is as follows:
///
/// - Procstar calls `new()` in the parent (main) process, before forking
///   the proc.  This method allocates any resources that must be accessible
///   both to Procstar and the proc.
///
/// - Procstar calls `in_child()` in the child process after forking but before
///   execing.  This method performs any additional allocations or cleanup
///   required in the child process.
///
/// - After forking, Procstar calls `in_parent()` in the parent (main) process.
///   This performs any cleanup required in the parent, and may also return an
///   async task that continues to deal with the fd.
///
impl SharedFdHandler {
    pub fn new(proc_id: &ProcId, fd: RawFd, spec: spec::Fd, pipes: &mut Pipes) -> Result<Self> {
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
                let pipe_fds = sys::pipe()?;
                FdHandler::CaptureMemory {
                    fd,
                    read_fd: pipe_fds.read,
                    write_fd: pipe_fds.write,
                    encoding,
                    buf: Vec::new(),
                    attached,
                }
            }

            spec::Fd::PipeWrite => FdHandler::PipeWrite {
                fd,
                pipe_write_fd: pipes.get_write_fd(proc_id, fd)?,
            },

            spec::Fd::PipeRead {
                proc_id: ref from_proc_id,
                fd: ref from_fd,
            } => FdHandler::PipeRead {
                fd,
                pipe_read_fd: pipes.get_read_fd(from_proc_id, parse_fd(from_fd)?)?,
            },
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

            FdHandler::PipeWrite { pipe_write_fd, .. } => {
                // Only the child writes to the pipe.
                sys::close(pipe_write_fd)?;
                None
            }

            FdHandler::PipeRead { pipe_read_fd, .. } => {
                // Only the child reads from the pipe.
                sys::close(pipe_read_fd)?;
                None
            }
        })
    }

    pub fn in_child(self) -> Result<()> {
        match Rc::try_unwrap(self.0).unwrap().into_inner() {
            FdHandler::Inherit => {}

            FdHandler::Error { err } => return Err(err),

            FdHandler::Close { fd } => {
                sys::close(fd)?;
            }

            FdHandler::Dup { fd, dup_fd } => {
                if dup_fd != fd {
                    sys::dup2(dup_fd, fd)?;
                }
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
            }

            FdHandler::UnlinkedFile { fd, file_fd, .. } => {
                if file_fd != fd {
                    sys::dup2(file_fd, fd)?;
                    sys::close(file_fd)?;
                }
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
                sys::close(write_fd)?;
            }

            FdHandler::PipeWrite { fd, pipe_write_fd } => {
                // Attach the write pipe to the target fd.
                sys::dup2(pipe_write_fd, fd)?;
                sys::close(pipe_write_fd)?;
            }

            FdHandler::PipeRead { fd, pipe_read_fd } => {
                // Attach the read pipe to the target fd.
                sys::dup2(pipe_read_fd, fd)?;
                sys::close(pipe_read_fd)?;
            }
        }

        Ok(())
    }

    /// Returns data for the fd, if available, and whether it is UTF-8 text.
    pub fn get_data(&self, start: usize, stop: Option<usize>) -> Result<Option<FdData>> {
        Ok(match &*self.0.borrow() {
            FdHandler::UnlinkedFile {
                file_fd, encoding, ..
            } => Some(FdData {
                data: sys::read_from_file(*file_fd, start as u64, stop.map(|s| s as u64))?,
                encoding: *encoding,
            }),

            FdHandler::CaptureMemory { buf, encoding, .. } => {
                let stop = stop.unwrap_or_else(|| buf.len());
                Some(FdData {
                    data: buf[start..stop].to_vec(),
                    encoding: *encoding,
                })
            }

            _ => None,
        })
    }

    pub fn get_result(&self) -> Result<Option<FdRes>> {
        // FIXME: Should we provide more information here?
        Ok(match &*self.0.borrow() {
            FdHandler::UnlinkedFile {
                file_fd,
                encoding,
                attached,
                ..
            } => Some(if *attached {
                FdRes::from_bytes(*encoding, &sys::read_from_file(*file_fd, 0, None)?)
            } else {
                FdRes::detached(sys::get_file_length(*file_fd)?, *encoding)
            }),

            FdHandler::CaptureMemory {
                encoding,
                buf,
                attached,
                ..
            } => Some(if *attached {
                FdRes::from_bytes(*encoding, buf)
            } else {
                FdRes::detached(buf.len() as i64, *encoding)
            }),

            _ => None,
        })
    }
}

//------------------------------------------------------------------------------

pub fn make_fd_handler(
    proc_id: &ProcId,
    fd_str: String,
    spec: spec::Fd,
    pipes: &mut Pipes,
) -> (RawFd, SharedFdHandler) {
    let fd_num = parse_fd(&fd_str).unwrap(); // FIXME
    let handler = SharedFdHandler::new(proc_id, fd_num, spec, pipes)
        .unwrap_or_else(|err| SharedFdHandler(Rc::new(RefCell::new(FdHandler::Error { err }))));

    (fd_num, handler)
}
