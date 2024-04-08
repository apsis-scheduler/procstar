use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::vec::Vec;

use crate::fd::parse_fd;
use crate::procinfo::ProcessInfo;
use crate::procs::{start_procs, SharedProcs};
use crate::res::ProcRes;
use crate::sig::Signum;
use crate::spec;
use crate::spec::{FdName, ProcId};
use crate::sys::getenv;

//------------------------------------------------------------------------------

/// Information about the procstar instance.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ConnectionInfo {
    /// Connection ID.
    pub conn_id: String,
    /// Procstar group ID.
    pub group_id: String,
    /// Restricted executable, if any.
    pub restricted_exe: Option<String>,
}

//------------------------------------------------------------------------------

pub const DEFAULT_PORT: u32 = 59789;
pub const DEFAULT_GROUP: &str = "default";

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

    /// Requests the current result of a process, which may be running.
    ProcResultRequest { proc_id: ProcId },

    /// Requests sending a signal to a process.
    ProcSignalRequest { proc_id: ProcId, signum: Signum },

    /// Requests sending (part of) captured fd data for a process.
    ProcFdDataRequest { proc_id: ProcId, fd: FdName },

    /// Requests deletion of a process's records.  The process may not be
    /// running.
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

    /// An incoming message referenced a nonexistent proc ID.
    ProcUnknown { proc_id: ProcId },

    /// Registers or re-registers this instance.
    Register {
        conn: ConnectionInfo,
        proc: ProcessInfo,
        access_token: String,
    },

    /// The list of current proc IDs.
    ProcidList { proc_ids: Vec<ProcId> },

    /// The current result of a process, which may or may not have terminated.
    ProcResult { proc_id: ProcId, res: ProcRes },

    /// A portion of the captured fd data for a process.
    ProcFdData { proc_id: ProcId, fd: FdName, data: Vec<u8> },

    /// A process has been deleted.
    ProcDelete { proc_id: ProcId },
}

pub async fn handle_incoming(procs: &SharedProcs, msg: IncomingMessage) -> Option<OutgoingMessage> {
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
                Some(OutgoingMessage::ProcUnknown { proc_id })
            }
        }

        IncomingMessage::ProcSignalRequest {
            ref proc_id,
            signum,
        } => {
            if let Some(proc) = procs.get(&proc_id) {
                if let Err(err) = proc.borrow().send_signal(signum) {
                    Some(OutgoingMessage::IncomingMessageError {
                        msg,
                        err: err.to_string(),
                    })
                } else {
                    None
                }
            } else {
                Some(OutgoingMessage::ProcUnknown {
                    proc_id: proc_id.clone(),
                })
            }
        }

        IncomingMessage::ProcFdDataRequest { ref proc_id, fd: ref fd_name } => {
            match parse_fd(fd_name) {
                Ok(fd) => {
                    if let Some(proc) = procs.get(proc_id) {
                        match proc.borrow().get_fd_data(fd) {
                            Ok(Some((data, _is_text))) =>
                                Some(OutgoingMessage::ProcFdData { proc_id: proc_id.clone(), fd: fd_name.clone(), data }),
                            Ok(None) => Some(OutgoingMessage::IncomingMessageError { msg, err: "no fd data".to_owned() }),
                            Err(err) => Some(OutgoingMessage::IncomingMessageError { msg, err: err.to_string() }),
                        }
                    } else {
                        Some(OutgoingMessage::ProcUnknown { proc_id: proc_id.clone() })
                    }
                },
                Err(err) => Some(OutgoingMessage::IncomingMessageError { msg, err: err.to_string() }),
            }
        }

        IncomingMessage::ProcDeleteRequest { ref proc_id } => {
            match procs.remove_if_not_running(proc_id) {
                Ok(_) => None,
                Err(crate::err::Error::NoProcId(proc_id)) => {
                    Some(OutgoingMessage::ProcUnknown { proc_id })
                }
                Err(err) => Some(OutgoingMessage::IncomingMessageError {
                    msg,
                    err: err.to_string(),
                }),
            }
        }
    }
}
