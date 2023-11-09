use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::vec::Vec;

use crate::procinfo::ProcessInfo;
use crate::procs::{start_procs, SharedProcs};
use crate::res::ProcRes;
use crate::spec;
use crate::spec::ProcId;

//------------------------------------------------------------------------------

/// Information about the procstar instance.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ConnectionInfo {
    /// Connection ID.
    pub conn_id: String,
    /// Procstar group ID.
    pub group_id: String,
}

//------------------------------------------------------------------------------

pub const DEFAULT_PORT: u32 = 59789;
pub const DEFAULT_GROUP: &str = "default";

// FIXME: Elsewhere.
fn getenv(name: &str) -> Option<String> {
    std::env::vars()
        .filter(|(n, _)| n == name)
        .next()
        .map(|(_, v)| v)
}

/// Expands the agent server hostname.
pub fn expand_hostname(hostname: &Option<String>) -> Option<String> {
    return hostname.clone().or_else(|| getenv("PROCSTAR_AGENT_HOST"));
}

/// Expands the agent server port.
pub fn expand_port(port: Option<u32>) -> Option<u32> {
    port.or_else(|| {
        getenv("PROCSTAR_AGENT_PORT").map(|p| {
            p.parse()
                .unwrap_or_else(|err| panic!("invalid agent port: {}: {}", p, err))
        })
    })
}

pub fn get_default_conn_id() -> String {
    return uuid::Uuid::new_v4().to_string();
}

//------------------------------------------------------------------------------

#[derive(Debug)]
pub enum Error {
    Close,
    UnexpectedMessage(IncomingMessage),
    WrongMessageType(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match *self {
            Error::Close => f.write_str("closed"),
            Error::UnexpectedMessage(ref msg) => {
                f.write_fmt(format_args!("wrong message: {:?}", msg))
            }
            Error::WrongMessageType(ref msg) => {
                f.write_fmt(format_args!("wrong WebSocket message: {}", msg))
            }
        }
    }
}

//------------------------------------------------------------------------------

/// Incoming messages, originating from the websocket server.  Despite this
/// name, these messages are requests, to which we, the websocket client,
/// respond.
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum IncomingMessage {
    /// The instance was successfully registered.
    Registered,

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
        conn: ConnectionInfo,
        proc: ProcessInfo,
        access_token: String,
    },

    /// The list of current proc IDs.
    ProcidList { proc_ids: Vec<ProcId> },

    /// The current result of a process, which may or may not be complete.
    ProcResult { proc_id: ProcId, res: ProcRes },

    /// A process has been deleted.
    ProcDelete { proc_id: ProcId },
}

pub async fn handle_incoming(procs: SharedProcs, msg: IncomingMessage) -> Option<OutgoingMessage> {
    match msg {
        IncomingMessage::Registered => Some(OutgoingMessage::IncomingMessageError {
            msg,
            err: "unexpected".to_owned(),
        }),

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
