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
    ProcStart { procid: ProcId, spec: Proc },
    ProcidListRequest {},
    ProcResultRequest { procid: ProcId },
    ProcDeleteRequest { procid: ProcId },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum OutgoingMessage {
    // Outgoing message types.
    ProcidList { procids: Vec<ProcId> },
    ProcResult { procid: ProcId, res: ProcRes },
    ProcDelete { procid: ProcId },
}

pub async fn handle_incoming(
    procs: SharedRunningProcs,
    msg: IncomingMessage,
) -> Result<Option<OutgoingMessage>, Error> {
    match msg {
        IncomingMessage::ProcStart { procid, spec } => Ok(None),

        IncomingMessage::ProcidListRequest {} => {
            let procids = procs.get_proc_ids();
            Ok(Some(OutgoingMessage::ProcidList { procids }))
        }

        IncomingMessage::ProcResultRequest { procid } => Ok(None),
        IncomingMessage::ProcDeleteRequest { procid } => Ok(None),
    }
}
