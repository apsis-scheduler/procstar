extern crate libc;

use libc::{c_int, gid_t, pid_t, rusage, ssize_t, uid_t};
use std::ffi::CString;
use std::fs;
use std::io;
use std::io::{Read, Seek};
use std::mem::MaybeUninit;
use std::os::fd::{FromRawFd, IntoRawFd, RawFd};
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
        self.ptrs.as_ptr()
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
        let mut ptrs = strs.iter().map(|s| s.as_ptr()).collect::<Vec<_>>();
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
        _ => panic!("close returned {res}"),
    }
}

pub fn dup2(fd: fd_t, fd2: fd_t) -> io::Result<()> {
    let res = unsafe { libc::dup2(fd, fd2) };
    match res {
        -1 => Err(io::Error::last_os_error()),
        _ if res == fd2 => Ok(()),
        _ => panic!("dup2 returned {res}"),
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
    let env: Vec<String> = env.into_iter().map(|(n, v)| format!("{n}={v}")).collect();

    let exe = CString::new(exe).unwrap();
    let res = unsafe {
        libc::execve(
            exe.as_ptr(),
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
        _ => panic!("fork returned {child_pid}"),
    }
}

pub fn getpid() -> pid_t {
    unsafe { libc::getpid() }
}

pub fn getppid() -> pid_t {
    unsafe { libc::getppid() }
}

pub fn getuid() -> uid_t {
    unsafe { libc::getuid() }
}

pub fn geteuid() -> uid_t {
    unsafe { libc::geteuid() }
}

pub fn getgid() -> gid_t {
    unsafe { libc::getgid() }
}

pub fn getegid() -> gid_t {
    unsafe { libc::getegid() }
}

pub fn setsid() -> io::Result<pid_t> {
    match unsafe { libc::setsid() } {
        -1 => Err(io::Error::last_os_error()),
        pgid => Ok(pgid),
    }
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
        _ => panic!("mkstemp returned {fd}"),
    }
}

pub fn open(path: &Path, oflag: c_int, mode: c_int) -> io::Result<fd_t> {
    let fd = unsafe {
        let path = CString::new(path.to_str().unwrap()).unwrap();
        libc::open(path.as_ptr(), oflag, mode)
    };
    match fd {
        -1 => Err(io::Error::last_os_error()),
        _ if fd >= 0 => Ok(fd),
        _ => panic!("open returned {fd}"),
    }
}

pub struct RWPair<T> {
    pub read: T,
    pub write: T,
}

/// Creates an anonymous pipe.
///
/// Returns the read and write file descriptors of the ends of the pipe.
pub fn pipe() -> io::Result<RWPair<RawFd>> {
    let mut fildes: Vec<fd_t> = vec![-1, 2];
    match unsafe { libc::pipe(fildes.as_mut_ptr()) } {
        -1 => Err(io::Error::last_os_error()),
        0 => Ok(RWPair {
            read: fildes[0],
            write: fildes[1],
        }),
        ret => panic!("pipe returned {ret}"),
    }
}

pub fn read(fd: fd_t, buf: &mut [u8]) -> io::Result<usize> {
    match unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) } {
        -1 => Err(io::Error::last_os_error()),
        n if n >= 0 => Ok(n as usize),
        ret => panic!("read returned {ret}"),
    }
}

/// Reads the contents of a file, starting from position `start`, until position
/// `stop` or the end.
pub fn read_from_file(fd: RawFd, start: u64, stop: Option<u64>) -> io::Result<Vec<u8>> {
    // Wrap the fd in a file object, for convenience.  This takes ownership of the fd.
    let mut file = unsafe { fs::File::from_raw_fd(fd) };
    // Seek to front.
    file.seek(std::io::SeekFrom::Start(start))?;
    let buf = if let Some(stop) = stop {
        // Read to indicated stop position.
        let mut buf = vec![0; stop.saturating_sub(start) as usize];
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

pub fn get_file_length(fd: RawFd) -> io::Result<i64> {
    Ok(fstat(fd)?.st_size)
}

//------------------------------------------------------------------------------

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

/// Polls or blocks for termination of a process by pid.
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
            Err(err) => panic!("wait4 failed: {err}"),
        };
    }
}

//------------------------------------------------------------------------------

pub fn write(fd: fd_t, data: &[u8]) -> io::Result<ssize_t> {
    match unsafe { libc::write(fd, data.as_ptr() as *const libc::c_void, data.len()) } {
        -1 => Err(io::Error::last_os_error()),
        n if n >= 0 => Ok(n),
        ret => panic!("write returned {ret}"),
    }
}

//------------------------------------------------------------------------------

pub fn fstat(fd: fd_t) -> io::Result<libc::stat> {
    unsafe {
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        match libc::fstat(fd, stat.as_mut_ptr()) {
            -1 => Err(io::Error::last_os_error()),
            0 => Ok(stat.assume_init()),
            ret => panic!("fstat returned {ret}"),
        }
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
    if flags & libc::FD_CLOEXEC == 0 {
        fcntl_setfd(fd, flags | libc::FD_CLOEXEC)
    } else {
        // Already set; nothing to do.
        Ok(())
    }
}

//------------------------------------------------------------------------------

pub fn kill(pid: pid_t, signum: c_int) -> io::Result<()> {
    match unsafe { libc::kill(pid, signum) } {
        -1 => {
            let err = io::Error::last_os_error();
            // 3 = ESRCH
            Err(if err.raw_os_error() == Some(3) {
                io::Error::new(io::ErrorKind::NotFound, "No process or process group")
            } else {
                err
            })
        }
        0 => Ok(()),
        ret => panic!("kill returned {ret}"),
    }
}

//------------------------------------------------------------------------------

pub fn get_username() -> Option<String> {
    let euid = geteuid();
    let passwd = unsafe { libc::getpwuid(euid) };
    if passwd.is_null() {
        None
    } else {
        Some(
            unsafe { core::ffi::CStr::from_ptr((*passwd).pw_name) }
                .to_str()
                .unwrap()
                .to_owned(),
        )
    }
}

/// Returns the username of the effective UID, or the stringified effective UID
/// if this is not available.
pub fn get_username_safe() -> String {
    get_username().unwrap_or_else(|| geteuid().to_string())
}

pub fn get_groupname() -> Option<String> {
    let egid = getegid();
    let group = unsafe { libc::getgrgid(egid) };
    if group.is_null() {
        None
    } else {
        Some(
            unsafe { core::ffi::CStr::from_ptr((*group).gr_name) }
                .to_str()
                .unwrap()
                .to_owned(),
        )
    }
}

const HOST_NAME_MAX: usize = 64;

pub fn get_hostname() -> String {
    let mut buffer = vec![0u8; HOST_NAME_MAX];
    let ret = unsafe { libc::gethostname(buffer.as_mut_ptr() as *mut i8, buffer.len()) };
    match ret {
        0 => {
            // FIXME: Is this really the right way to do this??
            let len = buffer.iter().position(|&c| c == 0).unwrap();
            buffer.truncate(len);
            String::from_utf8(buffer).unwrap()
        }
        -1 => panic!("gethostname failed: {ret}"),
        _ => panic!("gethostname invalid ressult: {ret}"),
    }
}

//------------------------------------------------------------------------------

pub fn get_clk_tck() -> io::Result<i64> {
    let val = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if val == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(val)
    }
}

lazy_static! {
    pub static ref BOOT_TIME: std::time::SystemTime = {
        let uptime = std::fs::read_to_string("/proc/uptime");
        std::time::SystemTime::now()
            - std::time::Duration::from_secs_f64(
                uptime
                    .unwrap()
                    .trim()
                    .split(' ')
                    .next()
                    .unwrap()
                    .parse::<f64>()
                    .unwrap(),
            )
    };
    pub static ref PAGE_SIZE: u64 = (unsafe { libc::sysconf(libc::_SC_PAGESIZE) }) as u64;
}

//------------------------------------------------------------------------------

/// Returns the value of an environment variable.
pub fn getenv(name: &str) -> Option<String> {
    std::env::vars().find(|(n, _)| n == name).map(|(_, v)| v)
}
