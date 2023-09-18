use futures::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use std::rc::Rc;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::tungstenite::Error;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use url::Url;

use crate::procs::SharedRunningProcs;
use crate::proto;

//------------------------------------------------------------------------------

// FIXME: Elsewhere.
pub trait Notifier {
    fn notify(msg: &proto::OutgoingMessage);
}

//------------------------------------------------------------------------------

pub struct Connection {
    write: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
}

pub struct Handler {
    read: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
}

impl Handler {
    pub async fn run(self, procs: SharedRunningProcs) -> Result<(), Error> {
        // FIXME: Reconnect.
        self.read
            .for_each(|msg| async {
                let procs = procs.clone();
                if let Ok(msg) = msg {
                    match msg {
                        Message::Text(json) => {
                            match serde_json::from_str::<proto::IncomingMessage>(&json) {
                                Ok(msg) => {
                                    eprintln!("msg: {:?}", msg);
                                    proto::handle_incoming(procs, msg);
                                }
                                Err(err) => eprintln!("invalid JSON: {:?}: {}", err, json),
                            }
                        }
                        _ => eprintln!("unexpected ws msg: {}", msg),
                    }
                } else {
                    eprintln!("msg error: {:?}", msg.err());
                }
            })
            .await;

        Ok(())
    }
}

impl Connection {
    pub async fn connect(url: &Url) -> Result<(Rc<Connection>, Handler), Error> {
        eprintln!("connecting to {}", url);
        let (ws_stream, _) = connect_async(url).await?;
        eprintln!("connected");
        let (write, read) = ws_stream.split();
        Ok((Rc::new(Connection { write }), Handler { read }))
    }
}
