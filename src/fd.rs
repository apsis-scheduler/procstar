use std::cell::RefCell;
use std::os::fd::RawFd;
use std::rc::Rc;
use tokio::io::AsyncReadExt;
use tokio::task::JoinHandle;
use tokio_pipe::PipeRead;

use crate::err::Result;
use crate::spec;
use crate::sys;
use crate::sys::fd_t;

//------------------------------------------------------------------------------

pub trait Fd where Self: Sized {
    /// Called after fork() in parent process.
    fn in_parent(self) -> Result<Option<JoinHandle<()>>> {
        Ok(None)
    }

    /// Called after fork() in child process.
    fn in_child(&mut self) -> Result<Result<()>> {
        Ok(Ok(()))
    }
}

type SharedFd = Rc<RefCell<dyn Fd>>;

//------------------------------------------------------------------------------

pub struct MemoryCapture {
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
}

impl MemoryCapture {
    pub fn new(fd: fd_t, format: spec::CaptureFormat) -> Result<Self> {
        let (read_fd, write_fd) = sys::pipe()?;
        Ok(Self {
            fd,
            read_fd,
            write_fd,
            format,
            buf: Vec::new(),
        })
    }

    async fn run(mut self) {
        // TODO: Limit max len.
        // FIXME: Handle errors instead of unwrap().
        let mut read_pipe = PipeRead::from_raw_fd_checked(self.read_fd).unwrap();
        read_pipe.read_to_end(&mut self.buf).await.unwrap();
    }
}

impl Fd for MemoryCapture {
    fn in_parent(self) -> Result<Option<JoinHandle<()>>> {
        // Close the write end of the pipe.  Only the child writes.
        sys::close(self.write_fd)?;
        Ok(Some(tokio::task::spawn_local(self.run())))
    }

    /// Called in parent process after wait().
    // fn in_parent(self) -> io::Result<Option<FdRes>> {

    //     let mut buf = Vec::new();
    //     std::mem::swap(&mut buf, &mut self.buf);
    //     Ok(Some(FdRes::from_bytes(self.format, buf)))
    // }

    fn in_child(&mut self) -> Result<()> {
        sys::close(self.read_fd)?;
        sys::dup2(self.write_fd, self.fd)?;
        Ok(())
    }
}

