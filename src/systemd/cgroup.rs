// Should these include settable values of the controllers themselves?
// What is the hierarchy and naming convention?
// FIXME: error handling
// - create enum with various error types e.g. (OS error, parsing error)

use std::path::PathBuf;
use std::str::FromStr;

fn parse_scalar<T>(input: &str) -> T
where
    T: FromStr,
    <T as FromStr>::Err: std::fmt::Debug,
{
    // What do we do if there is a parse error? We could just return 0
    input.trim().parse::<T>().unwrap()
}

#[derive(Debug)]
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

#[derive(Debug)]
struct Memory {
    pub current: u64,
    pub peak: u64,
    // TODO: include memory.swap.*
}

impl Memory {
    pub fn read(cgroup_path: &PathBuf) -> Self {
        Memory {
            current: parse_scalar(
                &std::fs::read_to_string(cgroup_path.join("memory.current")).unwrap(),
            ),
            peak: parse_scalar(&std::fs::read_to_string(cgroup_path.join("memory.peak")).unwrap()),
        }
    }
}

#[derive(Debug)]
pub struct ResourceAccounting {
    cpu: Option<CPUStat>,
    memory: Option<Memory>,
}

impl ResourceAccounting {
    pub fn read(cgroup_path: &PathBuf) -> Self {
        ResourceAccounting {
            cpu: None,
            memory: Some(Memory::read(cgroup_path)),
        }
    }
}
