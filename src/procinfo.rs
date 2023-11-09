use libc::{gid_t, pid_t, uid_t};
use log::*;
use serde::{Deserialize, Serialize};
use std::vec::Vec;

use crate::sig;
use crate::sys::{
    get_clk_tck, get_groupname, get_hostname, get_username, getegid, geteuid, getgid, getpid,
    getppid, getuid, BOOT_TIME, PAGE_SIZE,
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

/// Process information from /proc/{pid}/stat.  See the `proc(5)` manpage.
///
/// Omits some fields that are unmaintained or inaccurate in recent Linux
/// versions, or that are uninteresting.
///
/// Converts some fields to not require context from the process host system
/// to interrpet:
/// - times converted to f64 secs
/// - time since boot converted to UTC time
/// - device numbers decoded to [maj, min]
/// - signal numbers converted to names
/// - signal masks converted to [names]
///
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
    // pub itrealvalue: i64,
    /// Start time of process.
    pub starttime: String,
    /// Virtual memory size.
    pub vsize: u64,
    /// Resident set size in bytes; inaccurate.
    pub rss: i64,
    /// Resident set size soft limit in bytes.
    pub rsslim: u64,
    // pub startcode: u64,
    // pub endcode: u64,
    // pub startstack: u64,
    // pub kstkesp: u64,
    // pub kstkeip: u64,
    /// Pending signals, excluding real-time signals.
    pub signal: Vec<String>,
    /// Blocked signals, excluding real-time signals.
    pub blocked: Vec<String>,
    /// Ignored signals, excluding real-time signals.
    pub sigignore: Vec<String>,
    /// Caught signals, excluding real-time signals.
    pub sigcatch: Vec<String>,
    /// Channel in which process is waiting.
    pub wchan: u64,
    // pub nswap: u64,
    // pub cnswap: u64,
    /// Signal sent to parent process on process death.
    pub exit_signal: Option<String>,
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
    // pub start_data: u64,
    // pub end_data: u64,
    // pub start_brk: u64,
    // pub arg_start: u64,
    // pub arg_end: u64,
    // pub env_start: u64,
    // pub env_end: u64,
    /// Process exit code.
    pub exit_code: i32,
}

fn expand_sig_bitmap(bits: i32) -> Vec<String> {
    let mut abbrevs = Vec::new();
    for s in 1..sig::NSIG {
        if bits & (1 << s) != 0 {
            abbrevs.push(sig::get_abbrev(s).unwrap_or("???"));
        }
    }
    abbrevs.into_iter().map(|s| s.to_owned()).collect()
}

impl ProcStat {
    /// Parses contents of a /proc/{pid}/stat file.  Panics on failure.
    pub fn parse(text: &str) -> Self {
        let (pid, text) = text.split_once(' ').unwrap();
        let pid = pid.parse::<pid_t>().unwrap();

        // FIXME: Convert some of these to proper units.

        let (_, text) = text.split_once('(').unwrap();
        let (comm, text) = text.split_once(')').unwrap();
        let comm = comm.to_owned();
        let text = text.trim();

        let mut parts = text.split(' ').into_iter();

        let state = parts.next().unwrap().to_owned();
        let ppid = parts.next().unwrap().parse().unwrap();
        let pgrp = parts.next().unwrap().parse().unwrap();
        let session = parts.next().unwrap().parse().unwrap();
        let tty_nr = parts.next().unwrap().parse::<u32>().unwrap();
        let tty_nr = (
            ((tty_nr >> 8) & 0xff) as u16,
            ((tty_nr >> 16) | (tty_nr & 0xff)) as u16,
        );
        let tpgid = parts.next().unwrap().parse().unwrap();
        let flags = parts.next().unwrap().parse().unwrap();
        let minflt = parts.next().unwrap().parse().unwrap();
        let cminflt = parts.next().unwrap().parse().unwrap();
        let majflt = parts.next().unwrap().parse().unwrap();
        let cmajflt = parts.next().unwrap().parse().unwrap();
        let tick = get_clk_tck().unwrap() as f64;
        let utime = parts.next().unwrap().parse::<u64>().unwrap() as f64 / tick;
        let stime = parts.next().unwrap().parse::<u64>().unwrap() as f64 / tick;
        let cutime = parts.next().unwrap().parse::<u64>().unwrap() as f64 / tick;
        let cstime = parts.next().unwrap().parse::<u64>().unwrap() as f64 / tick;
        let priority = parts.next().unwrap().parse().unwrap();
        let nice = parts.next().unwrap().parse().unwrap();
        let num_threads = parts.next().unwrap().parse().unwrap();
        let _itrealvalue = parts.next();
        let starttime = parts.next().unwrap().parse::<u64>().unwrap() as f64 / tick;
        let starttime = chrono::DateTime::<chrono::Utc>::from(
            *BOOT_TIME + std::time::Duration::from_secs_f64(starttime),
        )
        .to_rfc3339();
        let vsize = parts.next().unwrap().parse().unwrap();
        let rss = parts.next().unwrap().parse::<i64>().unwrap() * *PAGE_SIZE as i64;
        let rsslim = parts.next().unwrap().parse().unwrap();
        let _startcode = parts.next().unwrap();
        let _endcode = parts.next().unwrap();
        let _startstack = parts.next().unwrap();
        let _kstkesp = parts.next().unwrap();
        let _kstkeip = parts.next().unwrap();
        let signal = expand_sig_bitmap(parts.next().unwrap().parse().unwrap());
        let blocked = expand_sig_bitmap(parts.next().unwrap().parse().unwrap());
        let sigignore = expand_sig_bitmap(parts.next().unwrap().parse().unwrap());
        let sigcatch = expand_sig_bitmap(parts.next().unwrap().parse().unwrap());
        let wchan = parts.next().unwrap().parse().unwrap();
        let _nswap = parts.next();
        let _cswap = parts.next();
        let exit_signal = parts.next().unwrap().parse::<i32>().unwrap();
        let exit_signal = if exit_signal == 0 {
            None
        } else {
            Some(sig::get_abbrev(exit_signal).unwrap().to_owned())
        };
        let processor = parts.next().unwrap().parse().unwrap();
        let rt_priority = parts.next().unwrap().parse().unwrap();
        let policy = parts.next().unwrap().parse().unwrap();
        let delayacct_blkio_ticks = parts.next().unwrap().parse::<u64>().unwrap() as f64 / tick;
        let guest_time = parts.next().unwrap().parse::<u64>().unwrap() as f64 / tick;
        let cguest_time = parts.next().unwrap().parse().unwrap();
        let _start_data = parts.next().unwrap();
        let _end_data = parts.next().unwrap();
        let _start_brk = parts.next().unwrap();
        let _arg_start = parts.next().unwrap();
        let _arg_end = parts.next().unwrap();
        let _env_start = parts.next().unwrap();
        let _env_end = parts.next().unwrap();
        let exit_code = parts.next().unwrap().parse().unwrap();

        Self {
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
            // itrealvalue
            starttime,
            vsize,
            rss,
            rsslim,
            // startcode,
            // endcode,
            // startstack,
            // kstkesp,
            // kstkeip,
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
            // start_data,
            // end_data,
            // start_brk,
            // arg_start,
            // arg_end,
            // env_start,
            // env_end,
            exit_code,
        }
    }

    /// Loads process status from /proc/{pid}/stat.
    pub fn load(pid: pid_t) -> Result<Self, std::io::Error> {
        let path = format!("/proc/{}/stat", pid);
        let text = std::fs::read_to_string(path)?;
        Ok(Self::parse(&text))
    }

    pub fn load_or_log(pid: pid_t) -> Option<Self> {
        Self::load(pid)
            .or_else(|err| {
                error!("failed to load /proc/{}/stat: {}", pid, err);
                Err(err)
            })
            .ok()
    }
}

//------------------------------------------------------------------------------

/// Process memory usage information.  See the `proc (5)` manpage.
///
/// Values in bytes.  Omits values unused in recent Linux versions.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProcStatm {
    /// Total program size.
    size: u64,
    /// Resident set size; inaccurate.
    resident: u64,
    /// Size of resident shared pages; inaccurate.
    shared: u64,
    /// Size of program text.
    text: u64,
    // lib: u64,
    /// Size of program data + stack.
    data: u64,
    // dt: u64,
}

impl ProcStatm {
    /// Parses contents of a /proc/{pid}/statm file.  Panics on failure.
    pub fn parse(text: &str) -> Self {
        info!("ProcStatm::parse '{}'", text);
        let mut parts = text.trim().split(' ').into_iter();
        let size = parts.next().unwrap().parse::<u64>().unwrap() * *PAGE_SIZE;
        let resident = parts.next().unwrap().parse::<u64>().unwrap() * *PAGE_SIZE;
        let shared = parts.next().unwrap().parse::<u64>().unwrap() * *PAGE_SIZE;
        let text = parts.next().unwrap().parse::<u64>().unwrap() * *PAGE_SIZE;
        let _lib = parts.next().unwrap();
        let data = parts.next().unwrap().parse::<u64>().unwrap() * *PAGE_SIZE;
        let _dt = parts.next().unwrap();
        Self {
            size,
            resident,
            shared,
            text,
            // lib,
            data,
            // dt,
        }
    }

    /// Loads process memory usage from /proc/{pid}/statm.
    pub fn load(pid: pid_t) -> Result<Self, std::io::Error> {
        let path = format!("/proc/{}/statm", pid);
        let text = std::fs::read_to_string(path)?;
        Ok(Self::parse(&text))
    }

    pub fn load_or_log(pid: pid_t) -> Option<Self> {
        Self::load(pid)
            .or_else(|err| {
                error!("failed to load /proc/{}/statm: {}", pid, err);
                Err(err)
            })
            .ok()
    }
}
