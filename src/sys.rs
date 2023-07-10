extern crate libc;

use libc::{c_int, pid_t, rusage, ssize_t};
use std::ffi::CString;
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::RawFd;
use std::path::{Path, PathBuf};
use std::string::String;
use std::vec::Vec;

use crate::environ::Env;

#[allow(non_camel_case_types)]
pub type fd_t = c_int;

//------------------------------------------------------------------------------

/// C-style char* array, containing a NULL-terminated array of pointers to
/// nul-terminated strings.
struct CStringVec {
    // Nul-terminated strings.
    // FIXME: We need to keep this around as it stores the actual strings
    // pointed to by `ptrs`, but Rust doesn't know this.  Should figure out how
    // to tell it.
    #[allow(dead_code)]
    strs: Vec<CString>,

    // NULL-terminated vector of char* pointers.
    ptrs: Vec<*const i8>,
}

impl CStringVec {
    pub fn as_ptr(&self) -> *const *const i8 {
        self.ptrs.as_ptr() as *const *const i8
    }
}

impl<T> From<T> for CStringVec
where
    T: IntoIterator<Item = String>,
{
    fn from(strings: T) -> Self {
        // Build nul-terminated strings.
        let strs = strings
            .into_iter()
            .map(|s| CString::new(s).unwrap())
            .collect::<Vec<_>>();

        // Grab their pointers into an array.
        let mut ptrs = strs
            .iter()
            .map(|s| s.as_ptr() as *const i8)
            .collect::<Vec<_>>();
        // NULL-terminate the pointer array.
        ptrs.push(std::ptr::null());

        Self { strs, ptrs }
    }
}

//------------------------------------------------------------------------------

pub fn close(fd: fd_t) -> io::Result<()> {
    let res = unsafe { libc::close(fd) };
    match res {
        -1 => Err(io::Error::last_os_error()),
        0 => Ok(()),
        _ => panic!("close returned {}", res),
    }
}

pub fn dup2(fd: fd_t, fd2: fd_t) -> io::Result<()> {
    let res = unsafe { libc::dup2(fd, fd2) };
    match res {
        -1 => Err(io::Error::last_os_error()),
        _ if res == fd2 => Ok(()),
        _ => panic!("dup2 returned {}", res),
    }
}

pub fn execv(exe: String, args: Vec<String>) -> io::Result<()> {
    let res = unsafe { libc::execv(exe.as_ptr() as *const i8, CStringVec::from(args).as_ptr()) };
    // execv only returns on failure, with result -1.
    assert!(res == -1);
    Err(io::Error::last_os_error())
}

pub fn execve(exe: String, args: Vec<String>, env: Env) -> io::Result<()> {
    // Construct NAME=val strings for env vars.
    let env: Vec<String> = env
        .into_iter()
        .map(|(n, v)| format!("{}={}", n, v))
        .collect();

    let exe = CString::new(exe).unwrap();
    let res = unsafe {
        libc::execve(
            exe.as_ptr() as *const i8,
            CStringVec::from(args).as_ptr(),
            CStringVec::from(env).as_ptr(),
        )
    };
    // execve only returns on failure, with result -1.
    assert!(res == -1);
    Err(io::Error::last_os_error())
}

pub fn fork() -> io::Result<pid_t> {
    let child_pid = unsafe { libc::fork() };
    assert!(child_pid >= -1);
    match child_pid {
        -1 => Err(io::Error::last_os_error()),
        _ if child_pid >= 0 => Ok(child_pid),
        _ => panic!("fork returned {}", child_pid),
    }
}

pub fn getpid() -> pid_t {
    unsafe { libc::getpid() }
}

pub fn mkstemp(template: &str) -> io::Result<(PathBuf, fd_t)> {
    let path = CString::new(template)?;
    let (fd, path) = unsafe {
        let ptr = path.into_raw();
        (libc::mkstemp(ptr), CString::from_raw(ptr))
    };
    match fd {
        -1 => Err(io::Error::last_os_error()),
        _ if fd >= 0 => Ok((PathBuf::from(path.into_string().unwrap()), fd)),
        _ => panic!("mkstemp returned {}", fd),
    }
}

pub fn open(path: &Path, oflag: c_int, mode: c_int) -> io::Result<fd_t> {
    let fd = unsafe {
        let path = CString::new(path.to_str().unwrap()).unwrap();
        libc::open(path.as_ptr() as *const i8, oflag, mode)
    };
    match fd {
        -1 => Err(io::Error::last_os_error()),
        _ if fd >= 0 => Ok(fd),
        _ => panic!("open returned {}", fd),
    }
}

/// Creates an anonymous pipe.
///
/// Returns the read and write file descriptors of the ends of the pipe.
pub fn pipe() -> io::Result<(RawFd, RawFd)> {
    let mut fildes: Vec<fd_t> = vec![-1, 2];
    match unsafe { libc::pipe(fildes.as_mut_ptr()) } {
        -1 => Err(io::Error::last_os_error()),
        0 => Ok((fildes[0], fildes[1])),
        ret => panic!("pipe returned {}", ret),
    }
}

pub fn read(fd: fd_t, buf: &mut [u8]) -> io::Result<usize> {
    match unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) } {
        -1 => Err(io::Error::last_os_error()),
        n if n >= 0 => Ok(n as usize),
        ret => panic!("read returned {}", ret),
    }
}

pub type WaitInfo = (pid_t, c_int, rusage);

/// Performs a (possibly) blocking wait if `block`; else returns immediately.
/// Returns `Ok(None)` only if a nonblocking call doesn't find a process.
pub fn wait4(pid: pid_t, block: bool) -> io::Result<Option<WaitInfo>> {
    let mut status: c_int = 0;
    let mut usage = MaybeUninit::<rusage>::uninit();
    let options = if block { 0 } else { libc::WNOHANG };
    match unsafe { libc::wait4(pid, &mut status, options, usage.as_mut_ptr()) } {
        -1 => Err(io::Error::last_os_error()),
        0 => Ok(None),
        child_pid => Ok(Some((child_pid, status, unsafe { usage.assume_init() }))),
    }
}

/// Polls or blocks for completion of a process by pid.
pub fn wait(pid: pid_t, block: bool) -> Option<WaitInfo> {
    loop {
        match wait4(pid, block) {
            Ok(Some(ti)) => {
                let (wait_pid, _, _) = ti;
                assert!(wait_pid == pid);
                return Some(ti);
            }
            Ok(None) => {
                if block {
                    panic!("wait4 empty result");
                } else {
                    return None;
                }
            }
            Err(ref err) if err.kind() == std::io::ErrorKind::Interrupted => {
                // wait4 interrupted, possibly by SIGCHLD.
                if block {
                    // Keep going.
                    continue;
                } else {
                    // Return, as the caller might want to do something.
                    return None;
                }
            }
            Err(err) => panic!("wait4 failed: {}", err),
        };
    }
}

//------------------------------------------------------------------------------

pub fn write(fd: fd_t, data: &[u8]) -> io::Result<ssize_t> {
    match unsafe { libc::write(fd, data.as_ptr() as *const libc::c_void, data.len()) } {
        -1 => Err(io::Error::last_os_error()),
        n if n >= 0 => Ok(n),
        ret => panic!("write returned {}", ret),
    }
}

//------------------------------------------------------------------------------

pub fn fcntl_getfd(fd: RawFd) -> io::Result<i32> {
    match unsafe { libc::fcntl(fd, libc::F_GETFD) } {
        -1 => Err(io::Error::last_os_error()),
        flags => Ok(flags),
    }
}

pub fn fcntl_setfd(fd: RawFd, flags: i32) -> io::Result<()> {
    match unsafe { libc::fcntl(fd, libc::F_SETFD, flags) } {
        -1 => Err(io::Error::last_os_error()),
        _ => Ok(()),
    }
}

pub fn set_cloexec(fd: RawFd) -> io::Result<()> {
    let flags = fcntl_getfd(fd)?;
    return if flags & libc::FD_CLOEXEC == 0 {
        fcntl_setfd(fd, flags | libc::FD_CLOEXEC)
    } else {
        // Already set; nothing to do.
        Ok(())
    };
}

//------------------------------------------------------------------------------

pub fn kill(pid: pid_t, signum: c_int) -> io::Result<()> {
    match unsafe { libc::kill(pid, signum) } {
        -1 => Err(io::Error::last_os_error()),
        0 => Ok(()),
        ret => panic!("kill returned {}", ret),
    }
}

