//! A stand-in for the systemd user manager, used by the cleanup fault tests.
//!
//! It serves only the handful of `org.freedesktop.systemd1` calls procstar
//! makes, and misbehaves on demand so that the agent's cleanup error paths can
//! be exercised without a real systemd: a stop job that fails, never finishes
//! or fails only the first time, a scope that stays failed, a reset that
//! errors, a property read that hangs.
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
use std::sync::Arc;
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
    /// Every stop job reports failure instead of completing.
    StopJobFails,
    /// Only the first stop job fails, so the retry can recover.
    StopFailsOnce,
    /// Stop jobs never finish, so no `JobRemoved` arrives.
    StopNeverCompletes,
    /// Stopping the slice never returns at all, so the overall cleanup budget
    /// expires after the scope result and the accounting were collected.
    SliceStopHangs,
    /// `GetUnit` never replies.
    GetUnitHangs,
    /// The first stop fails, forcing a retry, and the scope reports `oom-kill`
    /// on the first look and `success` afterwards, as systemd does once a
    /// failed unit has been reset.
    RetryAfterFailedScope,
}

impl Mode {
    fn from_env() -> Self {
        let name = std::env::var("FAKE_SYSTEMD_MODE").unwrap_or_default();
        match name.as_str() {
            "" | "ok" => Mode::Ok,
            "scope-gone" => Mode::ScopeGone,
            "scope-failed" => Mode::ScopeFailed,
            "reset-fails" => Mode::ResetFails,
            "stop-job-fails" => Mode::StopJobFails,
            "stop-fails-once" => Mode::StopFailsOnce,
            "stop-never-completes" => Mode::StopNeverCompletes,
            "slice-stop-hangs" => Mode::SliceStopHangs,
            "get-unit-hangs" => Mode::GetUnitHangs,
            "retry-after-failed-scope" => Mode::RetryAfterFailedScope,
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

struct State {
    mode: Mode,
    jobs: AtomicU32,
    stops: AtomicU32,
    lookups: AtomicU32,
}

impl State {
    /// True while the scope should look failed.  In the retry scenario it
    /// looks failed only on the first lookup, as a real scope does once the
    /// agent has reset it.
    fn scope_is_failed(&self) -> bool {
        match self.mode {
            Mode::ScopeFailed | Mode::ResetFails => true,
            Mode::RetryAfterFailedScope | Mode::SliceStopHangs => {
                self.lookups.load(Ordering::SeqCst) <= 1
            }
            _ => false,
        }
    }
}

struct Manager(Arc<State>);

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
        // Starting always works; these tests are about cleanup.
        let job = self.next_job();
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
        let state = &self.0;
        let nth = state.stops.fetch_add(1, Ordering::SeqCst);
        if state.mode == Mode::SliceStopHangs && name.ends_with(".slice") {
            // Never even reply, so the whole cleanup budget has to expire.
            std::future::pending::<()>().await;
        }
        let job = self.next_job();
        let result = match state.mode {
            Mode::StopNeverCompletes => None,
            Mode::StopJobFails => Some("failed"),
            Mode::StopFailsOnce | Mode::RetryAfterFailedScope if nth == 0 => Some("failed"),
            _ => Some("done"),
        };
        if let Some(result) = result {
            Self::job_removed(&emitter, 0, job.as_ref(), name, result)
                .await
                .ok();
        }
        Ok(job)
    }

    async fn get_unit(&self, _name: &str) -> Result<OwnedObjectPath, SystemdError> {
        let state = &self.0;
        if state.mode == Mode::GetUnitHangs {
            std::future::pending::<()>().await;
        }
        state.lookups.fetch_add(1, Ordering::SeqCst);
        match state.mode {
            // A scope that stopped cleanly is already unloaded.
            Mode::ScopeGone | Mode::Ok | Mode::StopFailsOnce => {
                Err(SystemdError::NoSuchUnit("Unit not loaded.".to_owned()))
            }
            _ => Ok(ObjectPath::try_from(UNIT_PATH).unwrap().into()),
        }
    }

    async fn reset_failed_unit(&self, _name: &str) -> Result<(), SystemdError> {
        match self.0.mode {
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
        let n = self.0.jobs.fetch_add(1, Ordering::SeqCst);
        ObjectPath::try_from(format!("/org/freedesktop/systemd1/job/{n}"))
            .unwrap()
            .into()
    }
}

/// The run's unit.  Procstar reads `ActiveState` and `Result` from the scope
/// and `ControlGroup` from the slice, so one object carries all three.
struct Unit(Arc<State>);

#[interface(name = "org.freedesktop.systemd1.Unit")]
impl Unit {
    #[zbus(property, name = "ActiveState")]
    async fn active_state(&self) -> String {
        if self.0.scope_is_failed() {
            "failed".to_owned()
        } else {
            "inactive".to_owned()
        }
    }
}

struct Scope(Arc<State>);

#[interface(name = "org.freedesktop.systemd1.Scope")]
impl Scope {
    #[zbus(property, name = "Result")]
    async fn result(&self) -> String {
        if self.0.scope_is_failed() {
            "oom-kill".to_owned()
        } else {
            "success".to_owned()
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
    let state = Arc::new(State {
        mode: Mode::from_env(),
        jobs: AtomicU32::new(0),
        stops: AtomicU32::new(0),
        lookups: AtomicU32::new(0),
    });
    let _conn = connection::Builder::session()?
        .name("org.freedesktop.systemd1")?
        .serve_at(MANAGER_PATH, Manager(state.clone()))?
        .serve_at(UNIT_PATH, Unit(state.clone()))?
        .serve_at(UNIT_PATH, Scope(state.clone()))?
        .serve_at(UNIT_PATH, Slice)?
        .build()
        .await?;
    println!("ready");
    std::future::pending::<()>().await;
    Ok(())
}
