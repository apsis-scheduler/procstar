use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::vec::Vec;

use crate::procs::{start_procs, SharedProcs};
use crate::res::ProcRes;
use crate::spec;
use crate::spec::ProcId;
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

/// Incoming messages, originating from the websocket server.  Despite this
/// name, these messages are requests, to which we, the websocket client,
/// respond.
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum IncomingMessage {
    /// Requests new processes to be started.  `specs` maps proc IDs to process
    /// specs.  Proc IDs may not already be in use.
    ProcStart { specs: BTreeMap<ProcId, spec::Proc> },

    /// Requests a list of current proc IDs.
    ProcidListRequest {},

    /// Requests the current result of a process, which may or may not be
    /// complete.
    ProcResultRequest { proc_id: ProcId },

    /// Requests deletion of a process's records.  The process must be complete.
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

//------------------------------------------------------------------------------

/// Outgoing messages, originating here and sent to the websocket server.
/// Despite this naming, these messages are primarily responses to requests
/// originating with the server.
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum OutgoingMessage {
    /// An incoming message could not be processed.
    IncomingMessageError { msg: IncomingMessage, err: String },

    /// Registers or re-registers this instance.
    Register {
        conn_id: String,
        group: String,
        info: InstanceInfo,
    },

    /// A new process has been created.
    ProcNew { proc_id: ProcId },

    /// The list of current proc IDs.
    ProcidList { proc_ids: Vec<ProcId> },

    /// The current result of a process, which may or may not be complete.
    ProcResult { proc_id: ProcId, res: ProcRes },

    /// A process has been deleted.
    ProcDelete { proc_id: ProcId },
}

pub async fn handle_incoming(procs: SharedProcs, msg: IncomingMessage) -> Option<OutgoingMessage> {
    match msg {
        IncomingMessage::ProcStart { ref specs } => {
            if let Err(err) = start_procs(specs, procs) {
                Some(OutgoingMessage::IncomingMessageError {
                    msg,
                    err: err.to_string(),
                })
            } else {
                None
            }
        }

        IncomingMessage::ProcidListRequest {} => {
            let proc_ids = procs.get_proc_ids::<Vec<_>>();
            Some(OutgoingMessage::ProcidList { proc_ids })
        }

        IncomingMessage::ProcResultRequest { ref proc_id } => {
            if let Some(proc) = procs.get(&proc_id) {
                let proc_id = proc_id.clone();
                let res = proc.borrow().to_result();
                Some(OutgoingMessage::ProcResult { proc_id, res })
            } else {
                let proc_id = proc_id.clone();
                Some(OutgoingMessage::IncomingMessageError {
                    msg,
                    err: format!("no such proc id: {}", proc_id),
                })
            }
        }

        IncomingMessage::ProcDeleteRequest { ref proc_id } => {
            match procs.remove_if_complete(proc_id) {
                Ok(_) => None,
                Err(err) => Some(OutgoingMessage::IncomingMessageError {
                    msg,
                    err: err.to_string(),
                }),
            }
        }
    }
}
