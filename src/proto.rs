use serde::{Deserialize, Serialize};
use std::vec::Vec;

use crate::procs::SharedRunningProcs;
use crate::res::ProcRes;
use crate::spec::{Proc, ProcId};

//------------------------------------------------------------------------------

#[derive(Debug)]
pub enum Error {
    Connection(tungstenite::error::Error),
    Json(serde_json::Error),
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
    ProcStart { proc_id: ProcId, spec: Proc },
    ProcidListRequest {},
    ProcResultRequest { proc_id: ProcId },
    ProcDeleteRequest { proc_id: ProcId },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum OutgoingMessage {
    // Outgoing message types.
    Connect { name: String, group: String },
    ProcidList { proc_ids: Vec<ProcId> },
    ProcResult { proc_id: ProcId, res: ProcRes },
    ProcDelete { proc_id: ProcId },
}

pub async fn handle_incoming(
    procs: SharedRunningProcs,
    msg: IncomingMessage,
) -> Result<Option<OutgoingMessage>, Error> {
    match msg {
        IncomingMessage::ProcStart { proc_id, spec } => Ok(None),

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

        IncomingMessage::ProcDeleteRequest { proc_id } => Ok(None),
    }
}
