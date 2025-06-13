// TODO: load other interesting accounting files

use log::error;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};

fn load_scalar(path: &PathBuf) -> Result<String, std::io::Error> {
    Ok(std::fs::read_to_string(path)?.trim().to_owned())
}

fn load_flat_keyed(path: &PathBuf) -> Result<HashMap<String, String>, std::io::Error> {
    let contents = std::fs::read_to_string(path)?.trim().to_owned();
    Ok(contents
        .split_whitespace()
        .collect::<Vec<_>>()
        .chunks(2)
        .filter_map(|pair| (pair.len() == 2).then(|| (pair[0].to_owned(), pair[1].to_owned())))
        .collect())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Pids {
    pub current: u64,
    pub peak: u64,
}

impl Pids {
    pub fn load(cgroup_path: &PathBuf) -> Result<Self, std::io::Error> {
        let load_scalar = |filename| load_scalar(&cgroup_path.join(filename));
        Ok(Self {
            current: load_scalar("pids.current")?.parse().unwrap(),
            peak: load_scalar("pids.peak")?.parse().unwrap(),
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
    pub fn load(cgroup_path: &PathBuf) -> Result<Self, std::io::Error> {
        let mapping = load_flat_keyed(&cgroup_path.join("cpu.stat"))?;
        Ok(Self {
            usage_usec: mapping.get("usage_usec").unwrap().parse().unwrap(),
            user_usec: mapping.get("user_usec").unwrap().parse().unwrap(),
            system_usec: mapping.get("system_usec").unwrap().parse().unwrap(),
            // optional, only available if cpu controller is enabled
            nr_periods: mapping.get("nr_periods").map(|val| val.parse().unwrap()),
            nr_throttled: mapping.get("nr_throttled").map(|val| val.parse().unwrap()),
            throttled_usec: mapping
                .get("throttled_usec")
                .map(|val| val.parse().unwrap()),
            nr_bursts: mapping.get("nr_bursts").map(|val| val.parse().unwrap()),
            burst_usec: mapping.get("burst_usec").map(|val| val.parse().unwrap()),
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
    pub fn load(cgroup_path: &PathBuf) -> Result<Self, std::io::Error> {
        let load_scalar = |filename| load_scalar(&cgroup_path.join(filename));

        Ok(Memory {
            current: load_scalar("memory.current")?.parse().unwrap(),
            peak: load_scalar("memory.peak")?.parse().unwrap(),
            swap_current: load_scalar("memory.swap.current")?.parse().unwrap(),
            swap_peak: load_scalar("memory.swap.peak")?.parse().unwrap(),
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
    pub fn load(cgroup_path: &PathBuf) -> Result<Self, std::io::Error> {
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
