//! Minimal D-Bus interface proxy for `org.freedesktop.systemd1.Scope`.
//!
//! Only the properties procstar needs are declared here; see
//! `org.freedesktop.systemd1.Scope.xml` for the full interface.
use zbus::proxy;
#[proxy(
    interface = "org.freedesktop.systemd1.Scope",
    default_service = "org.freedesktop.systemd1"
)]
pub trait Scope {
    /// ControlGroup property
    #[zbus(property, name = "ControlGroup")]
    fn control_group(&self) -> zbus::Result<String>;

    /// Result property, e.g. "success", "oom-kill", "timeout", "resources".
    #[zbus(property, name = "Result")]
    fn result(&self) -> zbus::Result<String>;
}
