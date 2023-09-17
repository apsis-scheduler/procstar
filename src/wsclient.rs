use futures_util::{StreamExt, SinkExt};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Error;
use tokio_tungstenite::tungstenite::protocol::Message;
use url::Url;

use crate::procs::SharedRunningProcs;
use crate::proto;

//------------------------------------------------------------------------------

pub async fn run_ws(procs: SharedRunningProcs, url: &Url) -> Result<(), Error> {
    eprintln!("connecting to {}", url);
    let (ws_stream, _) = connect_async(url).await?;
    eprintln!("connected");
    let (mut ws_write, ws_read) = ws_stream.split();

    // FIXME: Reconnect.
    ws_read.for_each(|msg| async {
        if let Ok(msg) = msg {
            match msg {
                Message::Text(json) => 
                    if let Ok(msg) = serde_json::from_str::<proto::Message>(&json) {
                        // Dispatch.
                        eprintln!("got msg: {:?}", msg);
                    } else {
                        eprintln!("invalid JSON: {}", json);
                    },
                _ => eprintln!("unexpected ws msg: {}", msg),
            }
        } else {
            eprintln!("msg error: {:?}", msg.err());
        }
    }).await;

    Ok(())
}

