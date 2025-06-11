use super::cgroup::ResourceAccounting;
use super::manager::{ManagerProxy, UnitProperty};
use super::slice::SliceProxy;
use std::path::PathBuf;
use std::rc::Rc;
use uuid::Uuid;

// need a way stop a slice
// Do we need a UnitProxy also to get unit specific properties
// FIXME: error handling
// Create an enum with zbus:Error and cgroup accounting error

// TODO: make this portable
const CGROUP_ROOT: &str = "/sys/fs/cgroup";

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
        self.manager_proxy
            .start_transient_unit(&name, "replace", properties, &[])
            .await?;
        Ok(name)
    }

    // FIXME: generalize to any unit type?
    pub async fn get_slice_cgroup_path(&self, name: &str) -> Result<PathBuf, zbus::Error> {
        let unit_path = self.manager_proxy.get_unit(name).await?;
        let slice_proxy = SliceProxy::new(&self.connection, &unit_path).await?;
        let cgroup_rel = slice_proxy.control_group().await?;
        Ok(PathBuf::from(CGROUP_ROOT).join(cgroup_rel.trim_start_matches("/")))
    }

    pub async fn read_cgroup_accounting(
        &self,
        name: &str,
    ) -> Result<ResourceAccounting, zbus::Error> {
        let cgroup_path = self.get_slice_cgroup_path(name).await?;
        Ok(ResourceAccounting::read(&cgroup_path))
    }

    pub async fn stop(&self, name: &str) -> Result<(), zbus::Error> {
        self.manager_proxy.stop_unit(name, "replace").await?;
        Ok(())
    }
}

pub type SharedSystemdClient = Rc<SystemdClient> ;
