//! # D-Bus interface proxy for: `org.freedesktop.systemd1.Unit`
//!
//! This code was generated by `zbus-xmlgen` `5.1.0` from D-Bus introspection data.
//! Source: `org.freedesktop.systemd1.Slice.xml`.
//!
//! You may prefer to adapt it, instead of using it verbatim.

#![allow(clippy::type_complexity)]
//!
//! More information can be found in the [Writing a client proxy] section of the zbus
//! documentation.
//!
//! This type implements the [D-Bus standard interfaces], (`org.freedesktop.DBus.*`) for which the
//! following zbus API can be used:
//!
//! * [`zbus::fdo::PeerProxy`]
//! * [`zbus::fdo::IntrospectableProxy`]
//! * [`zbus::fdo::PropertiesProxy`]
//!
//! Consequently `zbus-xmlgen` did not generate code for the above interfaces.
//!
//! [Writing a client proxy]: https://dbus2.github.io/zbus/client.html
//! [D-Bus standard interfaces]: https://dbus.freedesktop.org/doc/dbus-specification.html#standard-interfaces,
use zbus::proxy;
#[proxy(
    interface = "org.freedesktop.systemd1.Unit",
    default_service = "org.freedesktop.systemd1"
)]
pub trait Unit {
    /// Clean method
    fn clean(&self, mask: &[&str]) -> zbus::Result<()>;

    /// EnqueueJob method
    #[allow(clippy::too_many_arguments)]
    fn enqueue_job(
        &self,
        job_type: &str,
        job_mode: &str,
    ) -> zbus::Result<(
        u32,
        zbus::zvariant::OwnedObjectPath,
        String,
        zbus::zvariant::OwnedObjectPath,
        String,
        Vec<(
            u32,
            zbus::zvariant::OwnedObjectPath,
            String,
            zbus::zvariant::OwnedObjectPath,
            String,
        )>,
    )>;

    /// Freeze method
    fn freeze(&self) -> zbus::Result<()>;

    /// Kill method
    fn kill(&self, whom: &str, signal: i32) -> zbus::Result<()>;

    /// Ref method
    fn ref_(&self) -> zbus::Result<()>;

    /// Reload method
    fn reload(&self, mode: &str) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;

    /// ReloadOrRestart method
    fn reload_or_restart(&self, mode: &str) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;

    /// ReloadOrTryRestart method
    fn reload_or_try_restart(&self, mode: &str) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;

    /// ResetFailed method
    fn reset_failed(&self) -> zbus::Result<()>;

    /// Restart method
    fn restart(&self, mode: &str) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;

    /// SetProperties method
    fn set_properties(
        &self,
        runtime: bool,
        properties: &[&(&str, &zbus::zvariant::Value<'_>)],
    ) -> zbus::Result<()>;

    /// Start method
    fn start(&self, mode: &str) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;

    /// Stop method
    fn stop(&self, mode: &str) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;

    /// Thaw method
    fn thaw(&self) -> zbus::Result<()>;

    /// TryRestart method
    fn try_restart(&self, mode: &str) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;

    /// Unref method
    fn unref(&self) -> zbus::Result<()>;

    /// AccessSELinuxContext property
    #[zbus(property, name = "AccessSELinuxContext")]
    fn access_selinux_context(&self) -> zbus::Result<String>;

    /// ActivationDetails property
    #[zbus(property)]
    fn activation_details(&self) -> zbus::Result<Vec<(String, String)>>;

    /// ActiveEnterTimestamp property
    #[zbus(property)]
    fn active_enter_timestamp(&self) -> zbus::Result<u64>;

    /// ActiveEnterTimestampMonotonic property
    #[zbus(property)]
    fn active_enter_timestamp_monotonic(&self) -> zbus::Result<u64>;

    /// ActiveExitTimestamp property
    #[zbus(property)]
    fn active_exit_timestamp(&self) -> zbus::Result<u64>;

    /// ActiveExitTimestampMonotonic property
    #[zbus(property)]
    fn active_exit_timestamp_monotonic(&self) -> zbus::Result<u64>;

    /// ActiveState property
    #[zbus(property)]
    fn active_state(&self) -> zbus::Result<String>;

    /// After property
    #[zbus(property)]
    fn after(&self) -> zbus::Result<Vec<String>>;

    /// AllowIsolate property
    #[zbus(property)]
    fn allow_isolate(&self) -> zbus::Result<bool>;

    /// AssertResult property
    #[zbus(property)]
    fn assert_result(&self) -> zbus::Result<bool>;

    /// AssertTimestamp property
    #[zbus(property)]
    fn assert_timestamp(&self) -> zbus::Result<u64>;

    /// AssertTimestampMonotonic property
    #[zbus(property)]
    fn assert_timestamp_monotonic(&self) -> zbus::Result<u64>;

    /// Asserts property
    #[zbus(property)]
    fn asserts(&self) -> zbus::Result<Vec<(String, bool, bool, String, i32)>>;

    /// Before property
    #[zbus(property)]
    fn before(&self) -> zbus::Result<Vec<String>>;

    /// BindsTo property
    #[zbus(property)]
    fn binds_to(&self) -> zbus::Result<Vec<String>>;

    /// BoundBy property
    #[zbus(property)]
    fn bound_by(&self) -> zbus::Result<Vec<String>>;

    /// CanClean property
    #[zbus(property)]
    fn can_clean(&self) -> zbus::Result<Vec<String>>;

    /// CanFreeze property
    #[zbus(property)]
    fn can_freeze(&self) -> zbus::Result<bool>;

    /// CanIsolate property
    #[zbus(property)]
    fn can_isolate(&self) -> zbus::Result<bool>;

    /// CanReload property
    #[zbus(property)]
    fn can_reload(&self) -> zbus::Result<bool>;

    /// CanStart property
    #[zbus(property)]
    fn can_start(&self) -> zbus::Result<bool>;

    /// CanStop property
    #[zbus(property)]
    fn can_stop(&self) -> zbus::Result<bool>;

    /// CollectMode property
    #[zbus(property)]
    fn collect_mode(&self) -> zbus::Result<String>;

    /// ConditionResult property
    #[zbus(property)]
    fn condition_result(&self) -> zbus::Result<bool>;

    /// ConditionTimestamp property
    #[zbus(property)]
    fn condition_timestamp(&self) -> zbus::Result<u64>;

    /// ConditionTimestampMonotonic property
    #[zbus(property)]
    fn condition_timestamp_monotonic(&self) -> zbus::Result<u64>;

    /// Conditions property
    #[zbus(property)]
    fn conditions(&self) -> zbus::Result<Vec<(String, bool, bool, String, i32)>>;

    /// ConflictedBy property
    #[zbus(property)]
    fn conflicted_by(&self) -> zbus::Result<Vec<String>>;

    /// Conflicts property
    #[zbus(property)]
    fn conflicts(&self) -> zbus::Result<Vec<String>>;

    /// ConsistsOf property
    #[zbus(property)]
    fn consists_of(&self) -> zbus::Result<Vec<String>>;

    /// DefaultDependencies property
    #[zbus(property)]
    fn default_dependencies(&self) -> zbus::Result<bool>;

    /// Description property
    #[zbus(property)]
    fn description(&self) -> zbus::Result<String>;

    /// Documentation property
    #[zbus(property)]
    fn documentation(&self) -> zbus::Result<Vec<String>>;

    /// DropInPaths property
    #[zbus(property)]
    fn drop_in_paths(&self) -> zbus::Result<Vec<String>>;

    /// FailureAction property
    #[zbus(property)]
    fn failure_action(&self) -> zbus::Result<String>;

    /// FailureActionExitStatus property
    #[zbus(property)]
    fn failure_action_exit_status(&self) -> zbus::Result<i32>;

    /// Following property
    #[zbus(property)]
    fn following(&self) -> zbus::Result<String>;

    /// FragmentPath property
    #[zbus(property)]
    fn fragment_path(&self) -> zbus::Result<String>;

    /// FreezerState property
    #[zbus(property)]
    fn freezer_state(&self) -> zbus::Result<String>;

    /// Id property
    #[zbus(property)]
    fn id(&self) -> zbus::Result<String>;

    /// IgnoreOnIsolate property
    #[zbus(property)]
    fn ignore_on_isolate(&self) -> zbus::Result<bool>;

    /// InactiveEnterTimestamp property
    #[zbus(property)]
    fn inactive_enter_timestamp(&self) -> zbus::Result<u64>;

    /// InactiveEnterTimestampMonotonic property
    #[zbus(property)]
    fn inactive_enter_timestamp_monotonic(&self) -> zbus::Result<u64>;

    /// InactiveExitTimestamp property
    #[zbus(property)]
    fn inactive_exit_timestamp(&self) -> zbus::Result<u64>;

    /// InactiveExitTimestampMonotonic property
    #[zbus(property)]
    fn inactive_exit_timestamp_monotonic(&self) -> zbus::Result<u64>;

    /// InvocationID property
    #[zbus(property, name = "InvocationID")]
    fn invocation_id(&self) -> zbus::Result<Vec<u8>>;

    /// Job property
    #[zbus(property)]
    fn job(&self) -> zbus::Result<(u32, zbus::zvariant::OwnedObjectPath)>;

    /// JobRunningTimeoutUSec property
    #[zbus(property, name = "JobRunningTimeoutUSec")]
    fn job_running_timeout_usec(&self) -> zbus::Result<u64>;

    /// JobTimeoutAction property
    #[zbus(property)]
    fn job_timeout_action(&self) -> zbus::Result<String>;

    /// JobTimeoutRebootArgument property
    #[zbus(property)]
    fn job_timeout_reboot_argument(&self) -> zbus::Result<String>;

    /// JobTimeoutUSec property
    #[zbus(property, name = "JobTimeoutUSec")]
    fn job_timeout_usec(&self) -> zbus::Result<u64>;

    /// JoinsNamespaceOf property
    #[zbus(property)]
    fn joins_namespace_of(&self) -> zbus::Result<Vec<String>>;

    /// LoadError property
    #[zbus(property)]
    fn load_error(&self) -> zbus::Result<(String, String)>;

    /// LoadState property
    #[zbus(property)]
    fn load_state(&self) -> zbus::Result<String>;

    /// Markers property
    #[zbus(property)]
    fn markers(&self) -> zbus::Result<Vec<String>>;

    /// Names property
    #[zbus(property)]
    fn names(&self) -> zbus::Result<Vec<String>>;

    /// NeedDaemonReload property
    #[zbus(property)]
    fn need_daemon_reload(&self) -> zbus::Result<bool>;

    /// OnFailure property
    #[zbus(property)]
    fn on_failure(&self) -> zbus::Result<Vec<String>>;

    /// OnFailureJobMode property
    #[zbus(property)]
    fn on_failure_job_mode(&self) -> zbus::Result<String>;

    /// OnFailureOf property
    #[zbus(property)]
    fn on_failure_of(&self) -> zbus::Result<Vec<String>>;

    /// OnSuccess property
    #[zbus(property)]
    fn on_success(&self) -> zbus::Result<Vec<String>>;

    /// OnSuccessJobMode property
    #[zbus(property)]
    fn on_success_job_mode(&self) -> zbus::Result<String>;

    /// OnSuccessOf property
    #[zbus(property)]
    fn on_success_of(&self) -> zbus::Result<Vec<String>>;

    /// PartOf property
    #[zbus(property)]
    fn part_of(&self) -> zbus::Result<Vec<String>>;

    /// Perpetual property
    #[zbus(property)]
    fn perpetual(&self) -> zbus::Result<bool>;

    /// PropagatesReloadTo property
    #[zbus(property)]
    fn propagates_reload_to(&self) -> zbus::Result<Vec<String>>;

    /// PropagatesStopTo property
    #[zbus(property)]
    fn propagates_stop_to(&self) -> zbus::Result<Vec<String>>;

    /// RebootArgument property
    #[zbus(property)]
    fn reboot_argument(&self) -> zbus::Result<String>;

    /// Refs property
    #[zbus(property)]
    fn refs(&self) -> zbus::Result<Vec<String>>;

    /// RefuseManualStart property
    #[zbus(property)]
    fn refuse_manual_start(&self) -> zbus::Result<bool>;

    /// RefuseManualStop property
    #[zbus(property)]
    fn refuse_manual_stop(&self) -> zbus::Result<bool>;

    /// ReloadPropagatedFrom property
    #[zbus(property)]
    fn reload_propagated_from(&self) -> zbus::Result<Vec<String>>;

    /// RequiredBy property
    #[zbus(property)]
    fn required_by(&self) -> zbus::Result<Vec<String>>;

    /// Requires property
    #[zbus(property)]
    fn requires(&self) -> zbus::Result<Vec<String>>;

    /// RequiresMountsFor property
    #[zbus(property)]
    fn requires_mounts_for(&self) -> zbus::Result<Vec<String>>;

    /// Requisite property
    #[zbus(property)]
    fn requisite(&self) -> zbus::Result<Vec<String>>;

    /// RequisiteOf property
    #[zbus(property)]
    fn requisite_of(&self) -> zbus::Result<Vec<String>>;

    /// SliceOf property
    #[zbus(property)]
    fn slice_of(&self) -> zbus::Result<Vec<String>>;

    /// SourcePath property
    #[zbus(property)]
    fn source_path(&self) -> zbus::Result<String>;

    /// StartLimitAction property
    #[zbus(property)]
    fn start_limit_action(&self) -> zbus::Result<String>;

    /// StartLimitBurst property
    #[zbus(property)]
    fn start_limit_burst(&self) -> zbus::Result<u32>;

    /// StartLimitIntervalUSec property
    #[zbus(property, name = "StartLimitIntervalUSec")]
    fn start_limit_interval_usec(&self) -> zbus::Result<u64>;

    /// StateChangeTimestamp property
    #[zbus(property)]
    fn state_change_timestamp(&self) -> zbus::Result<u64>;

    /// StateChangeTimestampMonotonic property
    #[zbus(property)]
    fn state_change_timestamp_monotonic(&self) -> zbus::Result<u64>;

    /// StopPropagatedFrom property
    #[zbus(property)]
    fn stop_propagated_from(&self) -> zbus::Result<Vec<String>>;

    /// StopWhenUnneeded property
    #[zbus(property)]
    fn stop_when_unneeded(&self) -> zbus::Result<bool>;

    /// SubState property
    #[zbus(property)]
    fn sub_state(&self) -> zbus::Result<String>;

    /// SuccessAction property
    #[zbus(property)]
    fn success_action(&self) -> zbus::Result<String>;

    /// SuccessActionExitStatus property
    #[zbus(property)]
    fn success_action_exit_status(&self) -> zbus::Result<i32>;

    /// Transient property
    #[zbus(property)]
    fn transient(&self) -> zbus::Result<bool>;

    /// TriggeredBy property
    #[zbus(property)]
    fn triggered_by(&self) -> zbus::Result<Vec<String>>;

    /// Triggers property
    #[zbus(property)]
    fn triggers(&self) -> zbus::Result<Vec<String>>;

    /// UnitFilePreset property
    #[zbus(property)]
    fn unit_file_preset(&self) -> zbus::Result<String>;

    /// UnitFileState property
    #[zbus(property)]
    fn unit_file_state(&self) -> zbus::Result<String>;

    /// UpheldBy property
    #[zbus(property)]
    fn upheld_by(&self) -> zbus::Result<Vec<String>>;

    /// Upholds property
    #[zbus(property)]
    fn upholds(&self) -> zbus::Result<Vec<String>>;

    /// WantedBy property
    #[zbus(property)]
    fn wanted_by(&self) -> zbus::Result<Vec<String>>;

    /// Wants property
    #[zbus(property)]
    fn wants(&self) -> zbus::Result<Vec<String>>;
}
