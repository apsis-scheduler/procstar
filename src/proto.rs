use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::vec::Vec;

use crate::procs::{start_procs, SharedProcs};
use crate::res::ProcRes;
use crate::spec::{Input, Proc, ProcId};
use crate::sys;

//------------------------------------------------------------------------------

pub const DEFAULT_GROUP: &str = "default";

pub fn get_default_conn_id() -> String {
    return uuid::Uuid::new_v4().to_string();
}

//------------------------------------------------------------------------------

#[derive(Debug)]
pub enum Error {
    Connection(tungstenite::error::Error),
    Close,
    Json(serde_json::Error),
    WrongMessageType(String),
}

impl From<tungstenite::error::Error> for Error {
    fn from(err: tungstenite::error::Error) -> Error {
        Error::Connection(err)
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Error {
        Error::Json(err)
    }
}

//------------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum IncomingMessage {
    // Incomding message types.
    ProcStart { procs: BTreeMap<ProcId, Proc> },
    ProcidListRequest {},
    ProcResultRequest { proc_id: ProcId },
    ProcDeleteRequest { proc_id: ProcId },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InstanceInfo {
    hostname: String,
    username: String,
    pid: u32,
}

impl InstanceInfo {
    pub fn new() -> Self {
        let hostname = sys::get_hostname();
        let username = sys::get_username();
        let pid = sys::getpid() as u32;
        Self {
            hostname,
            username,
            pid,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum OutgoingMessage {
    // Outgoing message types.
    Register {
        conn_id: String,
        group: String,
        info: InstanceInfo,
    },
    ProcNew {
        proc_id: ProcId,
    },
    ProcidList {
        proc_ids: Vec<ProcId>,
    },
    ProcResult {
        proc_id: ProcId,
        res: ProcRes,
    },
    ProcDelete {
        proc_id: ProcId,
    },
}

pub async fn handle_incoming(
    procs: SharedProcs,
    msg: IncomingMessage,
) -> Result<Option<OutgoingMessage>, Error> {
    match msg {
        IncomingMessage::ProcStart { procs: procs_ } => {
            start_procs(Input { procs: procs_ }, procs).await;
            Ok(None)
        }

        IncomingMessage::ProcidListRequest {} => {
            let proc_ids = procs.get_proc_ids();
            Ok(Some(OutgoingMessage::ProcidList { proc_ids }))
        }

        IncomingMessage::ProcResultRequest { proc_id } => {
            if let Some(proc) = procs.get(&proc_id) {
                let res = proc.borrow().to_result();
                Ok(Some(OutgoingMessage::ProcResult { proc_id, res }))
            } else {
                // FIXME: How do we indicate protocol errors??
                eprintln!("no such proc id: {}", proc_id);
                Ok(None)
            }
        }

        IncomingMessage::ProcDeleteRequest { proc_id } => {
            match procs.remove_if_complete(&proc_id) {
                Ok(_) => Ok(None),
                Err(err) => {
                    // FIXME: How do we indicate protocol errors??
                    eprintln!("can't delete: {}", err);
                    Ok(None)
                }
            }
        }
    }
}
