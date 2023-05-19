use crate::err::{Error, Result};
use crate::fdio;
use crate::res::FdRes;
use crate::sel;
use crate::input;
use crate::sys;
use crate::sys::fd_t;
use libc;
use std::io;
use std::io::Read;
use std::io::Seek;
use std::os::unix::io::FromRawFd;
use std::path::PathBuf;

//------------------------------------------------------------------------------

pub fn parse_fd(fd: &str) -> std::result::Result<fd_t, std::num::ParseIntError> {
    match fd {
        "stdin" => Ok(0),
        "stdout" => Ok(1),
        "stderr" => Ok(2),
        _ => fd.parse::<fd_t>(),
    }
}

pub fn get_fd_name(fd: fd_t) -> String {
    match fd {
        0 => "stdin".to_string(),
        1 => "stdout".to_string(),
        2 => "stderr".to_string(),
        _ => fd.to_string(),
    }
}

// FIXME: Generalize: split out R/W/RW from file creation flags.
fn get_oflags(flags: &input::OpenFlag, fd: fd_t) -> libc::c_int {
    use input::OpenFlag::*;
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

pub trait Fd {
    fn get_fd(&self) -> fd_t;

    /// Called after fork(), in parent process.
    fn set_up_in_parent(&mut self) -> io::Result<Option<&mut dyn sel::Read>> {
        Ok(None)
    }

    /// Called after fork(), in child process.
    fn set_up_in_child(&mut self) -> io::Result<()> {
        Ok(())
    }

    /// Called in parent process after wait().
    // FIXME: Return something that becomes JSON null in result.
    fn clean_up_in_parent(&mut self) -> io::Result<Option<FdRes>> {
        Ok(None)
    }
}

//------------------------------------------------------------------------------

struct Inherit {
    fd: fd_t,
}

impl Fd for Inherit {
    fn get_fd(&self) -> fd_t {
        self.fd
    }
}

impl Inherit {
    fn new(fd: fd_t) -> Inherit {
        Inherit { fd }
    }
}

//------------------------------------------------------------------------------

struct Close {
    fd: fd_t,
}

impl Close {
    fn new(fd: fd_t) -> Close {
        Close { fd }
    }
}

impl Fd for Close {
    fn get_fd(&self) -> fd_t {
        self.fd
    }

    fn set_up_in_child(&mut self) -> io::Result<()> {
        sys::close(self.fd)?;
        Ok(())
    }
}

//------------------------------------------------------------------------------

struct File {
    fd: fd_t,
    path: PathBuf,
    oflags: libc::c_int,
    mode: libc::c_int,
}

impl File {
    fn new(fd: fd_t, path: PathBuf, flags: input::OpenFlag, mode: libc::c_int) -> File {
        File {
            fd,
            path,
            oflags: get_oflags(&flags, fd),
            mode,
        }
    }
}

impl Fd for File {
    fn get_fd(&self) -> fd_t {
        self.fd
    }

    fn set_up_in_child(&mut self) -> io::Result<()> {
        let file_fd = sys::open(&self.path, self.oflags, self.mode)?;
        sys::dup2(file_fd, self.fd)?;
        sys::close(file_fd)?;
        Ok(())
    }

    fn clean_up_in_parent(&mut self) -> io::Result<Option<FdRes>> {
        Ok(Some(FdRes::File {
            path: self.path.clone(),
        }))
    }
}

//------------------------------------------------------------------------------

struct Dup {
    fd: fd_t,
    /// File descriptor that will be duplicated.
    dup_fd: fd_t,
}

impl Dup {
    fn new(fd: fd_t, dup_fd: fd_t) -> Dup {
        Dup { fd, dup_fd }
    }
}

impl Fd for Dup {
    fn get_fd(&self) -> fd_t {
        self.fd
    }

    fn set_up_in_child(&mut self) -> io::Result<()> {
        sys::dup2(self.dup_fd, self.fd)?;
        Ok(())
    }

    // FIXME: Insert path into result.
}

//------------------------------------------------------------------------------

struct TempFileCapture {
    fd: fd_t,
    tmp_fd: fd_t,
    format: input::CaptureFormat,
}

// FIXME: Template.
const TMP_TEMPLATE: &str = "/tmp/ir-capture-XXXXXXXXXXXX";

impl TempFileCapture {
    fn new(fd: fd_t, format: input::CaptureFormat) -> Result<TempFileCapture> {
        let (tmp_path, tmp_fd) = sys::mkstemp(TMP_TEMPLATE)?;
        std::fs::remove_file(tmp_path)?;
        Ok(TempFileCapture { fd, tmp_fd, format })
    }
}

impl Fd for TempFileCapture {
    fn get_fd(&self) -> fd_t {
        self.fd
    }

    fn set_up_in_child(&mut self) -> io::Result<()> {
        sys::dup2(self.tmp_fd, self.fd)?;
        sys::close(self.tmp_fd)?;
        self.tmp_fd = -1;
        Ok(())
    }

    fn clean_up_in_parent(&mut self) -> io::Result<Option<FdRes>> {
        let mut file = unsafe {
            let file = std::fs::File::from_raw_fd(self.tmp_fd);
            self.tmp_fd = -1;
            file
        };
        file.seek(std::io::SeekFrom::Start(0))?;
        let mut reader = std::io::BufReader::new(file);

        let mut bytes: Vec<u8> = Vec::new();
        let _size = reader.read_to_end(&mut bytes)?;

        Ok(Some(FdRes::from_bytes(self.format, bytes)))
    }
}

//------------------------------------------------------------------------------

pub struct MemoryCapture {
    /// Proc-visible fd.
    fd: fd_t,

    /// Read end of the pipe.
    read_fd: fd_t,

    /// Write end of the pipe.
    write_fd: fd_t,

    /// Format for output.
    format: input::CaptureFormat,

    /// Captured output.
    buf: Vec<u8>,
}

impl MemoryCapture {
    fn new(fd: fd_t, format: input::CaptureFormat) -> Result<MemoryCapture> {
        let (read_fd, write_fd) = sys::pipe()?;
        Ok(MemoryCapture {
            fd,
            read_fd,
            write_fd,
            format,
            buf: Vec::new(),
        })
    }
}

impl sel::Read for MemoryCapture {
    fn get_fd(&self) -> fd_t {
        self.read_fd
    }

    fn read(&mut self) -> bool {
        const SIZE: usize = 1024;
        match fdio::read_into_vec(self.read_fd, &mut self.buf, SIZE) {
            Ok(_) => false,
            Err(Error::Eof) => true,
            Err(err) => panic!("error: {}", err),
        }
    }
}

impl Fd for MemoryCapture {
    fn get_fd(&self) -> fd_t {
        self.fd
    }

    fn set_up_in_child(&mut self) -> io::Result<()> {
        sys::close(self.read_fd)?;
        sys::dup2(self.write_fd, self.fd)?;
        Ok(())
    }

    fn set_up_in_parent(&mut self) -> io::Result<Option<&mut dyn sel::Read>> {
        // Close the write end of the pipe.  Only the child writes.
        sys::close(self.write_fd)?;
        Ok(Some(self))
    }

    /// Called in parent process after wait().
    fn clean_up_in_parent(&mut self) -> io::Result<Option<FdRes>> {
        let mut buf = Vec::new();
        std::mem::swap(&mut buf, &mut self.buf);
        Ok(Some(FdRes::from_bytes(self.format, buf)))
    }
}

//------------------------------------------------------------------------------

pub fn create_fd(fd: fd_t, fd_input: &input::Fd) -> Result<Box<dyn Fd>> {
    Ok(match fd_input {
        input::Fd::Inherit => Box::new(Inherit::new(fd)),
        input::Fd::Close => Box::new(Close::new(fd)),
        input::Fd::Null { flags } => Box::new(File::new(fd, PathBuf::from("/dev/null"), *flags, 0)),
        input::Fd::File { path, flags, mode } => {
            Box::new(File::new(fd, path.to_path_buf(), *flags, *mode))
        }
        input::Fd::Dup { fd: other_fd } => Box::new(Dup::new(fd, *other_fd)),
        input::Fd::Capture { mode, format } => match mode {
            input::CaptureMode::TempFile => Box::new(TempFileCapture::new(fd, *format)?),
            input::CaptureMode::Memory => Box::new(MemoryCapture::new(fd, *format)?),
        },
    })
}
