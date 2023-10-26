use libc::{gid_t, pid_t, uid_t};
use serde::{Deserialize, Serialize};

use crate::sys::{
    get_groupname, get_hostname, get_username, getegid, geteuid, getgid, getpid, getuid,
};

// FIXME: Use getpwuid and getgrgid to implement username and groupname.

//------------------------------------------------------------------------------

/// General information about a process.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProcessInfo {
    /// Process ID.
    pub pid: pid_t,
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
        let uid = getuid();
        let euid = geteuid();
        let username = get_username();
        let gid = getgid();
        let egid = getegid();
        let groupname = get_groupname();
        let hostname = get_hostname();
        Self {
            pid,
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
