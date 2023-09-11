use serde::{Deserialize, Serialize};
use std::vec::Vec;

use crate::spec::{Proc, ProcId};
use crate::res::ProcRes;

//------------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum Message {
    // Incomding message types.
    ProcStart {procid: ProcId, spec: Proc},
    ProcidListRequest {},
    ProcResultRequest {procid: ProcId},
    ProcDeleteRequest {procid: ProcId},

    // Outgoing message types.
    ProcidList {procids: Vec<ProcId>},
    ProcResult {procid: ProcId, res: ProcRes},
    ProcDelete {procid: ProcId},
}

