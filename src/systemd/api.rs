use super::manager::{ManagerProxy, UnitProperty};
use super::slice::SliceProxy;
use futures_util::StreamExt;
use log::debug;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::LazyLock;
use uuid::Uuid;

// TODO: make this portable
static CGROUP_ROOT: LazyLock<PathBuf> = LazyLock::new(|| PathBuf::from("/sys/fs/cgroup"));

#[derive(Debug)]
pub enum UnitType {
    Slice,
    Scope,
}

pub fn generate_unit_name(unit_type: UnitType) -> String {
    let uuid = Uuid::new_v4();
    let suffix = match unit_type {
        UnitType::Slice => "slice",
        UnitType::Scope => "scope",
    };
    format!("procstar{}.{}", uuid.as_simple(), suffix)
}

pub struct SystemdClient {
    connection: zbus::Connection,
    manager_proxy: ManagerProxy<'static>,
}

impl SystemdClient {
    pub async fn new() -> Result<Self, zbus::Error> {
        let connection = zbus::Connection::session().await?;
        Ok(Self {
            manager_proxy: ManagerProxy::new(&connection).await?,
            connection,
        })
    }

    pub async fn start_transient_unit(
        &self,
        unit_type: UnitType,
        properties: &[UnitProperty<'_>],
    ) -> Result<String, zbus::Error> {
        let name = generate_unit_name(unit_type);

        let mut removed_stream = self.manager_proxy.receive_job_removed().await?;

        // request to start unit
        let job_path = self
            .manager_proxy
            .start_transient_unit(&name, "replace", properties, &[])
            .await?;

        // wait for request to actually complete
        while let Some(job_removed) = removed_stream.next().await {
            let args = job_removed.args()?;
            if *job_path == *args.job() {
                break;
            }
        }
        Ok(name)
    }

    // FIXME: generalize to any unit type?
    pub async fn get_slice_cgroup_path(&self, name: &str) -> Result<PathBuf, zbus::Error> {
        let unit_path = self.manager_proxy.get_unit(name).await?;
        let slice_proxy = SliceProxy::new(&self.connection, &unit_path).await?;
        let cgroup_rel = slice_proxy.control_group().await?;
        Ok(CGROUP_ROOT.join(cgroup_rel.trim_start_matches("/")))
    }

    pub async fn stop(&self, name: &str) -> Result<(), zbus::Error> {
        self.manager_proxy.stop_unit(name, "replace").await?;
        Ok(())
    }
}

pub type SharedSystemdClient = Rc<SystemdClient>;

pub async fn maybe_connect() -> Option<SystemdClient> {
    let systemd = SystemdClient::new()
        .await
        .map_err(|err| {
            debug!("unable to create systemd client: {err}");
        })
        .ok()?;

    if !CGROUP_ROOT.join("cgroup.controllers").is_file() {
        debug!("cgroup v2 hierarchy not detected");
        return None;
    }
    Some(systemd)
}
