// TODO: load other interesting accounting files

use log::error;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt, io, path::PathBuf, str::FromStr};

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
        .map_err(|_| Error::Parse(path.file_name().map_or(PathBuf::from(""), PathBuf::from)))
}

fn load_flat_keyed<T: FromStr>(path: &PathBuf) -> Result<HashMap<String, T>, Error> {
    let contents = std::fs::read_to_string(path)?.trim().to_owned();
    let mut output = HashMap::new();

    let name = path.file_name().map_or(PathBuf::from(""), PathBuf::from);

    for line in contents.lines() {
        let mut parts = line.split_whitespace();
        if let (Some(k), Ok(v)) = (
            parts.next(),
            parts.next().ok_or(Error::Parse(name.clone()))?.parse::<T>(),
        ) {
            output.insert(k.to_owned(), v);
        } else {
            return Err(Error::Parse(name.clone()));
        }
    }

    return Ok(output);
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Pids {
    pub current: u64,
    pub peak: u64,
}

impl Pids {
    pub fn load(cgroup_path: &PathBuf) -> Result<Self, Error> {
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
    pub fn load(cgroup_path: &PathBuf) -> Result<Self, Error> {
        let stat_rel = PathBuf::from("cpu.stat");
        let mut mapping: HashMap<String, u64> = load_flat_keyed(&cgroup_path.join(&stat_rel))?;
        Ok(Self {
            usage_usec: mapping
                .remove("usage_usec")
                .ok_or(Error::Parse(stat_rel.clone()))?,
            user_usec: mapping
                .remove("user_usec")
                .ok_or(Error::Parse(stat_rel.clone()))?,
            system_usec: mapping
                .remove("system_usec")
                .ok_or(Error::Parse(stat_rel.clone()))?,
            // optional, only available if cpu controller is enabled
            nr_periods: mapping.remove("nr_periods"),
            nr_throttled: mapping.remove("nr_throttled"),
            throttled_usec: mapping.remove("throttled_usec"),
            nr_bursts: mapping.remove("nr_bursts"),
            burst_usec: mapping.remove("burst_usec"),
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Memory {
    pub current: u64,
    pub peak: u64,
    pub swap_current: u64,
    pub swap_peak: u64,
}

impl Memory {
    pub fn load(cgroup_path: &PathBuf) -> Result<Self, Error> {
        let load_scalar = |filename| load_scalar(&cgroup_path.join(filename));

        Ok(Memory {
            current: load_scalar("memory.current")?,
            peak: load_scalar("memory.peak")?,
            swap_current: load_scalar("memory.swap.current")?,
            swap_peak: load_scalar("memory.swap.peak")?,
        })
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CGroupAccounting {
    pids: Option<Pids>,
    cpu_stat: Option<CPUStat>,
    memory: Option<Memory>,
}

impl CGroupAccounting {
    pub fn load(cgroup_path: &PathBuf) -> Result<Self, Error> {
        Ok(CGroupAccounting {
            pids: Some(Pids::load(cgroup_path)?),
            cpu_stat: Some(CPUStat::load(cgroup_path)?),
            memory: Some(Memory::load(cgroup_path)?),
        })
    }

    pub fn load_or_log(cgroup_path: &PathBuf) -> Option<Self> {
        Self::load(cgroup_path)
            .or_else(|err| {
                error!("failed to load accounting for {}", cgroup_path.display());
                Err(err)
            })
            .ok()
    }
}
