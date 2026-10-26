//! A stand-in for the systemd user manager, used by the cleanup fault tests.
//!
//! It serves only the handful of `org.freedesktop.systemd1` calls procstar
//! makes, and misbehaves on demand so that the agent's cleanup error paths can
//! be exercised without a real systemd: a stop job that never finishes or
//! reports failure, a scope that stays failed, a reset that errors, a property
//! read that hangs.
//!
//! Run it on a private bus and point the agent at the same bus:
//!
//! ```shell
//! eval $(dbus-daemon --session --print-address --fork)
//! FAKE_SYSTEMD_MODE=stop-job-fails cargo run --example fake_systemd
//! ```
//!
//! It prints `ready` once it owns the name.  Not part of the shipped binary.

use std::sync::atomic::{AtomicU32, Ordering};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue};
use zbus::{connection, interface, DBusError};

const MANAGER_PATH: &str = "/org/freedesktop/systemd1";
const UNIT_PATH: &str = "/org/freedesktop/systemd1/unit/fake";

/// Errors with the names systemd uses, so that callers which special-case
/// `NoSuchUnit` see what they expect.
#[derive(Debug, DBusError)]
#[zbus(prefix = "org.freedesktop.systemd1")]
enum SystemdError {
    NoSuchUnit(String),
    UnitFailed(String),
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Everything works; the scope is already unloaded at cleanup.
    Ok,
    /// `GetUnit` reports the scope as gone.
    ScopeGone,
    /// The scope is loaded and failed; `ResetFailedUnit` succeeds.
    ScopeFailed,
    /// The scope is loaded and failed; `ResetFailedUnit` returns an error.
    ResetFails,
    /// Stop jobs report failure instead of completing.
    StopJobFails,
    /// Stop jobs never finish, so no `JobRemoved` arrives.
    StopNeverCompletes,
    /// `GetUnit` never replies.
    GetUnitHangs,
}

impl Mode {
    fn from_env() -> Self {
        match std::env::var("FAKE_SYSTEMD_MODE")
            .unwrap_or_default()
            .as_str()
        {
            "" | "ok" => Mode::Ok,
            "scope-gone" => Mode::ScopeGone,
            "scope-failed" => Mode::ScopeFailed,
            "reset-fails" => Mode::ResetFails,
            "stop-job-fails" => Mode::StopJobFails,
            "stop-never-completes" => Mode::StopNeverCompletes,
            "get-unit-hangs" => Mode::GetUnitHangs,
            other => panic!("unknown FAKE_SYSTEMD_MODE: {other}"),
        }
    }
}

/// A cgroup that exists and has the memory controller, so that accounting
/// reads succeed and we can check that evidence survives a cleanup fault.
fn real_control_group() -> String {
    let uid = unsafe { libc::getuid() };
    format!("/user.slice/user-{uid}.slice")
}

struct Manager {
    mode: Mode,
    jobs: AtomicU32,
}

#[interface(name = "org.freedesktop.systemd1.Manager")]
impl Manager {
    async fn start_transient_unit(
        &self,
        name: &str,
        _mode: &str,
        _properties: Vec<(String, OwnedValue)>,
        _aux: Vec<(String, Vec<(String, OwnedValue)>)>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        let job = self.next_job();
        // Starting always works; these tests are about cleanup.
        Self::job_removed(&emitter, 0, job.as_ref(), name, "done")
            .await
            .ok();
        Ok(job)
    }

    async fn stop_unit(
        &self,
        name: &str,
        _mode: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        let job = self.next_job();
        match self.mode {
            Mode::StopNeverCompletes => {
                // Return the job, but never report it finished.
            }
            Mode::StopJobFails => {
                Self::job_removed(&emitter, 0, job.as_ref(), name, "failed")
                    .await
                    .ok();
            }
            _ => {
                Self::job_removed(&emitter, 0, job.as_ref(), name, "done")
                    .await
                    .ok();
            }
        }
        Ok(job)
    }

    async fn get_unit(&self, _name: &str) -> Result<OwnedObjectPath, SystemdError> {
        match self.mode {
            Mode::GetUnitHangs => std::future::pending().await,
            Mode::ScopeGone | Mode::Ok => {
                Err(SystemdError::NoSuchUnit("Unit not loaded.".to_owned()))
            }
            _ => Ok(ObjectPath::try_from(UNIT_PATH).unwrap().into()),
        }
    }

    async fn reset_failed_unit(&self, _name: &str) -> Result<(), SystemdError> {
        match self.mode {
            Mode::ResetFails => Err(SystemdError::UnitFailed("Cannot reset.".to_owned())),
            _ => Ok(()),
        }
    }

    #[zbus(signal)]
    async fn job_removed(
        emitter: &SignalEmitter<'_>,
        id: u32,
        job: ObjectPath<'_>,
        unit: &str,
        result: &str,
    ) -> zbus::Result<()>;
}

impl Manager {
    fn next_job(&self) -> OwnedObjectPath {
        let n = self.jobs.fetch_add(1, Ordering::SeqCst);
        ObjectPath::try_from(format!("/org/freedesktop/systemd1/job/{n}"))
            .unwrap()
            .into()
    }
}

/// The run's unit.  Procstar reads `ActiveState` and `Result` from the scope
/// and `ControlGroup` from the slice, so one object carries all three.
struct Unit {
    mode: Mode,
}

#[interface(name = "org.freedesktop.systemd1.Unit")]
impl Unit {
    #[zbus(property, name = "ActiveState")]
    async fn active_state(&self) -> String {
        match self.mode {
            Mode::ScopeFailed | Mode::ResetFails => "failed".to_owned(),
            _ => "inactive".to_owned(),
        }
    }
}

struct Scope {
    mode: Mode,
}

#[interface(name = "org.freedesktop.systemd1.Scope")]
impl Scope {
    #[zbus(property, name = "Result")]
    async fn result(&self) -> String {
        match self.mode {
            Mode::ScopeFailed | Mode::ResetFails => "oom-kill".to_owned(),
            _ => "success".to_owned(),
        }
    }

    #[zbus(property, name = "ControlGroup")]
    async fn control_group(&self) -> String {
        real_control_group()
    }
}

struct Slice;

#[interface(name = "org.freedesktop.systemd1.Slice")]
impl Slice {
    #[zbus(property, name = "ControlGroup")]
    async fn control_group(&self) -> String {
        real_control_group()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mode = Mode::from_env();
    let _conn = connection::Builder::session()?
        .name("org.freedesktop.systemd1")?
        .serve_at(
            MANAGER_PATH,
            Manager {
                mode,
                jobs: AtomicU32::new(0),
            },
        )?
        .serve_at(UNIT_PATH, Unit { mode })?
        .serve_at(UNIT_PATH, Scope { mode })?
        .serve_at(UNIT_PATH, Slice)?
        .build()
        .await?;
    println!("ready");
    std::future::pending::<()>().await;
    Ok(())
}
