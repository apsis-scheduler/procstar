use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// The process was forked but the process was not execed.
    Error,
    /// The process was forked and execed, and has not yet terminated.
    Running,
    /// The process was forked and execed and has terminated.
    Terminated,
}
