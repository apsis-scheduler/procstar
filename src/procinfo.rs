use libc::{gid_t, pid_t, uid_t};
use serde::{Deserialize, Serialize};

use crate::err::Error;
use crate::sys::{
    get_clk_tck, get_groupname, get_hostname, get_username, getegid, geteuid, getgid, getpid,
    getppid, getuid, BOOT_TIME,
};

// FIXME: Use getpwuid and getgrgid to implement username and groupname.

//------------------------------------------------------------------------------

/// General information about a process.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProcessInfo {
    /// Process ID.
    pub pid: pid_t,
    /// Parent process ID.
    pub ppid: pid_t,
    /// User ID.
    pub uid: uid_t,
    /// Effective user ID.
    pub euid: uid_t,
    /// Username corresponding to effective user ID.
    pub username: Option<String>,
    /// System group ID.
    pub gid: gid_t,
    /// Effective system group ID.
    pub egid: gid_t,
    /// Groupname corresponding to effective group ID.
    pub groupname: Option<String>,

    /// Canonical hostname.
    pub hostname: String,
}

impl ProcessInfo {
    /// Populates for the current process.
    pub fn new_self() -> Self {
        let pid = getpid();
        let ppid = getppid();
        let uid = getuid();
        let euid = geteuid();
        let username = get_username();
        let gid = getgid();
        let egid = getegid();
        let groupname = get_groupname();
        let hostname = get_hostname();
        Self {
            pid,
            ppid,
            uid,
            euid,
            username,
            gid,
            egid,
            groupname,
            hostname,
        }
    }
}

//------------------------------------------------------------------------------

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProcStat {
    /// Process ID.
    pub pid: pid_t,
    /// First 16 characters of executable basename.
    pub comm: String,
    /// Process state code.
    pub state: String,
    /// Process ID of parent process.
    pub ppid: pid_t,
    /// Group ID of process group.
    pub pgrp: gid_t,
    /// Session ID.
    pub session: u32, // FIXME
    /// Controlling terminal (major, minor) device number.
    pub tty_nr: (u16, u16),
    /// Group ID of foreground process group on controlling terminal.
    pub tpgid: gid_t,
    /// Kernel flags word.
    pub flags: u32,
    /// Number of minor faults from process.
    pub minflt: u64,
    /// Number of minor faults from awaited children.
    pub cminflt: u64,
    /// Number of major faults.
    pub majflt: u64,
    /// Number of major faults from awaited children.
    pub cmajflt: u64,
    /// Time in sec process has been scheduled in user mode.
    pub utime: f64,
    /// Time in sec process has been scheduled in kernel mode.
    pub stime: f64,
    /// Time in sec awaited children have been scheduled in user mode.
    pub cutime: f64,
    /// Time in sec awaited children have been scheduled in kernel mode.
    pub cstime: f64,
    /// Encoded process priority.
    pub priority: i64,
    /// Nice value.
    pub nice: i64,
    /// Number of threads in process.
    pub num_threads: i64,
    // itrealvalue omitted
    /// Start time of process.
    pub starttime: String,
    /// Virtual memory size.
    pub vsize: u64,
    // rss skipped
    /// Resident set size soft limit in bytes.
    pub rsslim: u64,
    pub startcode: u64,
    pub endcode: u64,
    pub startstack: u64,
    pub kstkesp: u64,
    pub kstkeip: u64,
    pub signal: u64,
    pub blocked: u64,
    pub sigignore: u64,
    pub sigcatch: u64,
    /// Channel in which process is waiting.
    pub wchan: u64,
    // nswap
    // cnswap
    /// Signal sent to parent process on process death.
    pub exit_signal: i32,
    /// CPU number last executed on.
    pub processor: i32,
    /// Real-time scheduling priority.
    pub rt_priority: u32,
    /// Scheduling policy.
    pub policy: u32,
    /// Aggregated block I/O delays.
    pub delayacct_blkio_ticks: f64,
    /// Guest time of process.
    pub guest_time: f64,
    /// Guest time of awaited children.
    pub cguest_time: i64,
    pub start_data: u64,
    pub end_data: u64,
    pub start_brk: u64,
    pub arg_start: u64,
    pub arg_end: u64,
    pub env_start: u64,
    pub env_end: u64,
    /// Process exit code.
    pub exit_code: i32,
}

impl ProcStat {
    pub fn parse(text: &str) -> Result<Self, Error> {
        let (pid, text) = text.split_once(' ').ok_or(Error::Eof)?;
        let pid = pid.parse::<pid_t>()?;

        // FIXME: Convert some of these to proper units.

        let (_, text) = text.split_once('(').ok_or(Error::Eof)?;
        let (comm, text) = text.split_once(')').ok_or(Error::Eof)?;
        let comm = comm.to_owned();
        let text = text.trim();

        let mut parts = text.split(' ').into_iter();

        let state = parts.next().ok_or(Error::Eof)?.to_owned();
        let ppid = parts.next().ok_or(Error::Eof)?.parse()?;
        let pgrp = parts.next().ok_or(Error::Eof)?.parse()?;
        let session = parts.next().ok_or(Error::Eof)?.parse()?;
        let tty_nr = parts.next().ok_or(Error::Eof)?.parse::<u32>()?;
        let tty_nr = (
            ((tty_nr >> 8) & 0xff) as u16,
            ((tty_nr >> 16) | (tty_nr & 0xff)) as u16,
        );
        let tpgid = parts.next().ok_or(Error::Eof)?.parse()?;
        let flags = parts.next().ok_or(Error::Eof)?.parse()?;
        let minflt = parts.next().ok_or(Error::Eof)?.parse()?;
        let cminflt = parts.next().ok_or(Error::Eof)?.parse()?;
        let majflt = parts.next().ok_or(Error::Eof)?.parse()?;
        let cmajflt = parts.next().ok_or(Error::Eof)?.parse()?;
        let tick = get_clk_tck().unwrap() as f64;
        let utime = parts.next().ok_or(Error::Eof)?.parse::<u64>()? as f64 / tick;
        let stime = parts.next().ok_or(Error::Eof)?.parse::<u64>()? as f64 / tick;
        let cutime = parts.next().ok_or(Error::Eof)?.parse::<u64>()? as f64 / tick;
        let cstime = parts.next().ok_or(Error::Eof)?.parse::<u64>()? as f64 / tick;
        let priority = parts.next().ok_or(Error::Eof)?.parse()?;
        let nice = parts.next().ok_or(Error::Eof)?.parse()?;
        let num_threads = parts.next().ok_or(Error::Eof)?.parse()?;
        let _itrealvalue = parts.next();
        let starttime = parts.next().ok_or(Error::Eof)?.parse::<u64>()? as f64 / tick;
        let starttime = chrono::DateTime::<chrono::Utc>::from(
            *BOOT_TIME + std::time::Duration::from_secs_f64(starttime),
        )
        .to_rfc3339();
        let vsize = parts.next().ok_or(Error::Eof)?.parse()?;
        let rsslim = parts.next().ok_or(Error::Eof)?.parse()?;
        let startcode = parts.next().ok_or(Error::Eof)?.parse()?;
        let endcode = parts.next().ok_or(Error::Eof)?.parse()?;
        let startstack = parts.next().ok_or(Error::Eof)?.parse()?;
        let kstkesp = parts.next().ok_or(Error::Eof)?.parse()?;
        let kstkeip = parts.next().ok_or(Error::Eof)?.parse()?;
        let signal = parts.next().ok_or(Error::Eof)?.parse()?;
        let blocked = parts.next().ok_or(Error::Eof)?.parse()?;
        let sigignore = parts.next().ok_or(Error::Eof)?.parse()?;
        let sigcatch = parts.next().ok_or(Error::Eof)?.parse()?;
        let wchan = parts.next().ok_or(Error::Eof)?.parse()?;
        let _nswap = parts.next();
        let _cswap = parts.next();
        let exit_signal = parts.next().ok_or(Error::Eof)?.parse()?;
        let processor = parts.next().ok_or(Error::Eof)?.parse()?;
        let rt_priority = parts.next().ok_or(Error::Eof)?.parse()?;
        let policy = parts.next().ok_or(Error::Eof)?.parse()?;
        let delayacct_blkio_ticks = parts.next().ok_or(Error::Eof)?.parse::<u64>()? as f64 / tick;
        let guest_time = parts.next().ok_or(Error::Eof)?.parse::<u64>()? as f64 / tick;
        let cguest_time = parts.next().ok_or(Error::Eof)?.parse()?;
        let start_data = parts.next().ok_or(Error::Eof)?.parse()?;
        let end_data = parts.next().ok_or(Error::Eof)?.parse()?;
        let start_brk = parts.next().ok_or(Error::Eof)?.parse()?;
        let arg_start = parts.next().ok_or(Error::Eof)?.parse()?;
        let arg_end = parts.next().ok_or(Error::Eof)?.parse()?;
        let env_start = parts.next().ok_or(Error::Eof)?.parse()?;
        let env_end = parts.next().ok_or(Error::Eof)?.parse()?;
        let exit_code = parts.next().ok_or(Error::Eof)?.parse()?;

        Ok(Self {
            pid,
            comm,
            state,
            ppid,
            pgrp,
            session,
            tty_nr,
            tpgid,
            flags,
            minflt,
            cminflt,
            majflt,
            cmajflt,
            utime,
            stime,
            cutime,
            cstime,
            priority,
            nice,
            num_threads,
            starttime,
            vsize,
            rsslim,
            startcode,
            endcode,
            startstack,
            kstkesp,
            kstkeip,
            signal,
            blocked,
            sigignore,
            sigcatch,
            wchan,
            exit_signal,
            processor,
            rt_priority,
            policy,
            delayacct_blkio_ticks,
            guest_time,
            cguest_time,
            start_data,
            end_data,
            start_brk,
            arg_start,
            arg_end,
            env_start,
            env_end,
            exit_code,
        })
    }

    /// Loads process status from /proc/{pid}/stat.
    pub fn load(pid: pid_t) -> Result<Self, Error> {
        let path = format!("/proc/{}/stat", pid);
        let text = std::fs::read_to_string(path)?;
        Ok(Self::parse(&text)?)
    }
}
