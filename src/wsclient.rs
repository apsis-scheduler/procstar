use futures_util::{StreamExt, SinkExt};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::protocol::Message;

use crate::procs::SharedRunningProcs;

//------------------------------------------------------------------------------

pub async fn run_ws(procs: SharedRunningProcs, addr: &str) {
    let url = url::Url::parse(addr).unwrap();
    eprintln!("connecting to {}", url);
    let (ws_stream, _) = connect_async(url).await.unwrap();
    eprintln!("connected");
    let (mut ws_write, ws_read) = ws_stream.split();
    let msg = Message::text("hello");
    ws_write.send(msg.into()).await.unwrap();
    ws_read.for_each(|msg| async {
        eprintln!("msg: {}", msg.unwrap());
    }).await;
}

