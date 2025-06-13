// Should these include settable values of the controllers themselves?
// What is the hierarchy and naming convention?
// FIXME: error handling
// - create enum with various error types e.g. (OS error, parsing error)

use log::error;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn load_scalar(path: &PathBuf) -> Result<String, std::io::Error> {
    Ok(std::fs::read_to_string(path)?.trim().to_owned())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CPUStat {
    pub usage_usec: u64,
    pub user_usec: u64,
    pub system_usec: u64,
    // below are only available when cpu controller is enabled
    pub nr_periods: Option<u64>,
    pub nr_throttled: Option<u64>,
    pub nr_throttled_usec: Option<u64>,
    pub nr_bursts: Option<u64>,
    pub burst_usec: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Memory {
    pub current: u64,
    pub peak: u64,
    pub swap_current: u64,
    pub swap_peak: u64,
    // TODO: include memory.swap.*
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
    cpu: Option<CPUStat>,
    memory: Option<Memory>,
}

impl CGroupAccounting {
    pub fn load(cgroup_path: &PathBuf) -> Result<Self, std::io::Error> {
        Ok(CGroupAccounting {
            cpu: None,
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
