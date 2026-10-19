// TODO: load other interesting accounting files

use log::{debug, error};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fmt, fs, io,
    path::{Path, PathBuf},
    str::FromStr,
    sync::LazyLock,
};

// TODO: make this portable
pub static CGROUP_ROOT: LazyLock<PathBuf> = LazyLock::new(|| PathBuf::from("/sys/fs/cgroup"));

pub enum Error {
    Io(std::io::Error),
    Parse(PathBuf),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Io(err) => err.fmt(f),
            Error::Parse(target) => write!(f, "failed to parse: {target:?}"),
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Error {
        Error::Io(err)
    }
}

fn load_scalar<T: FromStr>(path: &PathBuf) -> Result<T, Error> {
    std::fs::read_to_string(path)?
        .trim()
        .parse()
        .map_err(|_| Error::Parse(path.clone()))
}

fn load_flat_keyed<T: FromStr>(path: &PathBuf) -> Result<HashMap<String, T>, Error> {
    let contents = std::fs::read_to_string(path)?.trim().to_owned();
    let mut output = HashMap::new();

    for line in contents.lines() {
        let mut parts = line.split_whitespace();
        if let (Some(k), Ok(v)) = (
            parts.next(),
            parts.next().ok_or(Error::Parse(path.clone()))?.parse::<T>(),
        ) {
            output.insert(k.to_owned(), v);
        } else {
            return Err(Error::Parse(path.clone()));
        }
    }

    Ok(output)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Pids {
    pub current: u64,
    pub peak: u64,
}

impl Pids {
    pub fn load(cgroup_path: &Path) -> Result<Self, Error> {
        let load_scalar = |filename| load_scalar(&cgroup_path.join(filename));
        Ok(Self {
            current: load_scalar("pids.current")?,
            peak: load_scalar("pids.peak")?,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CPUStat {
    pub usage_usec: u64,
    pub user_usec: u64,
    pub system_usec: u64,
    // below are only available when cpu controller is enabled
    pub nr_periods: Option<u64>,
    pub nr_throttled: Option<u64>,
    pub throttled_usec: Option<u64>,
    pub nr_bursts: Option<u64>,
    pub burst_usec: Option<u64>,
}

impl CPUStat {
    pub fn load(cgroup_path: &Path) -> Result<Self, Error> {
        let path = cgroup_path.join("cpu.stat");
        let mut mapping: HashMap<String, u64> = load_flat_keyed(&path)?;
        Ok(Self {
            usage_usec: mapping
                .remove("usage_usec")
                .ok_or(Error::Parse(path.clone()))?,
            user_usec: mapping
                .remove("user_usec")
                .ok_or(Error::Parse(path.clone()))?,
            system_usec: mapping
                .remove("system_usec")
                .ok_or(Error::Parse(path.clone()))?,
            // optional, only available if cpu controller is enabled
            nr_periods: mapping.remove("nr_periods"),
            nr_throttled: mapping.remove("nr_throttled"),
            throttled_usec: mapping.remove("throttled_usec"),
            nr_bursts: mapping.remove("nr_bursts"),
            burst_usec: mapping.remove("burst_usec"),
        })
    }
}

/// Counters from `memory.events` / `memory.events.local`.  See the cgroup v2
/// documentation; `oom_kill` counts OOM victims in the cgroup (also from a
/// system-wide OOM), `oom` counts times this cgroup itself ran out of memory,
/// `max` counts times usage hit `memory.max`.
#[derive(Clone, Debug, Deserialize, Serialize)]
struct MemoryEvents {
    pub low: u64,
    pub high: u64,
    pub max: u64,
    pub oom: u64,
    pub oom_kill: u64,
    pub oom_group_kill: Option<u64>,
}

impl MemoryEvents {
    fn load(path: &PathBuf) -> Result<Self, Error> {
        let mut map: HashMap<String, u64> = load_flat_keyed(path)?;
        let mut take = |key: &str| map.remove(key).ok_or(Error::Parse(path.clone()));
        let low = take("low")?;
        let high = take("high")?;
        let max = take("max")?;
        let oom = take("oom")?;
        let oom_kill = take("oom_kill")?;
        let oom_group_kill = map.remove("oom_group_kill");
        Ok(Self {
            low,
            high,
            max,
            oom,
            oom_kill,
            oom_group_kill,
        })
    }
}

/// The effective `memory.max` of the cgroup: either unlimited (`"max"`) or a
/// byte count.  Absent when the file could not be read.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum MemoryMax {
    Unlimited(String),
    Bytes(u64),
}

impl MemoryMax {
    fn load(path: &PathBuf) -> Result<Self, Error> {
        let text = std::fs::read_to_string(path)?.trim().to_owned();
        if text == "max" {
            Ok(MemoryMax::Unlimited(text))
        } else {
            text.parse()
                .map(MemoryMax::Bytes)
                .map_err(|_| Error::Parse(path.clone()))
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Memory {
    pub current: u64,
    pub peak: u64,
    pub swap_current: u64,
    pub swap_peak: u64,
    /// Hierarchical event counters for this cgroup and its descendants.
    #[serde(default)]
    pub events: Option<MemoryEvents>,
    /// Event counters local to this cgroup.
    #[serde(default)]
    pub events_local: Option<MemoryEvents>,
    /// The effective `memory.max` at termination.
    #[serde(default)]
    pub max: Option<MemoryMax>,
}

impl Memory {
    pub fn load(cgroup_path: &Path) -> Result<Self, Error> {
        let load_scalar = |filename| load_scalar(&cgroup_path.join(filename));
        let optional = |name: &str, err: &Error| {
            debug!("{}: {}: {}", cgroup_path.display(), name, err);
        };

        let events = MemoryEvents::load(&cgroup_path.join("memory.events"))
            .inspect_err(|e| optional("memory.events", e))
            .ok();
        let events_local = MemoryEvents::load(&cgroup_path.join("memory.events.local"))
            .inspect_err(|e| optional("memory.events.local", e))
            .ok();
        let max = MemoryMax::load(&cgroup_path.join("memory.max"))
            .inspect_err(|e| optional("memory.max", e))
            .ok();

        Ok(Memory {
            current: load_scalar("memory.current")?,
            peak: load_scalar("memory.peak")?,
            swap_current: load_scalar("memory.swap.current")?,
            swap_peak: load_scalar("memory.swap.peak")?,
            events,
            events_local,
            max,
        })
    }
}

fn enabled_controllers(cgroup_path: &Path) -> Result<HashSet<String>, Error> {
    let control_file = if *cgroup_path == *CGROUP_ROOT {
        // in the root cgroup, cgroup.controllers reports all the controllers that can
        // be enabled, not necessarily the ones that are
        cgroup_path.join("cgroup.subtree_control")
    } else {
        // in non-root cgroups this corresponds the parent's cgroup.subtree_control and
        // does track which accounting and control files will be available
        cgroup_path.join("cgroup.controllers")
    };

    Ok(fs::read_to_string(control_file)?
        .split_whitespace()
        .map(|s| s.to_owned())
        .collect())
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CGroupAccounting {
    pids: Option<Pids>,
    cpu_stat: Option<CPUStat>,
    memory: Option<Memory>,
}

impl CGroupAccounting {
    pub fn load(cgroup_path: &Path) -> Result<Self, Error> {
        let controllers = enabled_controllers(cgroup_path)?;
        Ok(CGroupAccounting {
            pids: if controllers.contains("pids") {
                Some(Pids::load(cgroup_path)?)
            } else {
                None
            },
            memory: if controllers.contains("memory") {
                Some(Memory::load(cgroup_path)?)
            } else {
                None
            },
            // always load CPU because some cput.stat fields are available even when
            // the controller isn't enabled
            cpu_stat: Some(CPUStat::load(cgroup_path)?),
        })
    }

    pub fn load_or_log(cgroup_path: &Path) -> Option<Self> {
        Self::load(cgroup_path)
            .inspect_err(|_err| {
                error!("failed to load accounting for {}", cgroup_path.display());
            })
            .ok()
    }
}
