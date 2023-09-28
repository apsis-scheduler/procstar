use futures::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use std::cell::RefCell;
use std::rc::Rc;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::tungstenite::Error;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use url::Url;

use crate::procs::SharedRunningProcs;
use crate::proto;

//------------------------------------------------------------------------------

pub struct Connection {
    write: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
}

impl Connection {
    async fn send(&mut self, msg: proto::OutgoingMessage) -> Result<(), proto::Error> {
        let json = serde_json::to_vec(&msg)?;
        self.write.send(Message::Binary(json)).await?;
        Ok(())
    }
}

type SharedConnection = Rc<RefCell<Connection>>;

pub struct Handler {
    read: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    connection: SharedConnection,
}

impl Handler {
    async fn handle(
        connection: SharedConnection,
        procs: SharedRunningProcs,
        msg: Message,
    ) -> Result<(), proto::Error> {
        match msg {
            Message::Binary(json) => match serde_json::from_slice::<proto::IncomingMessage>(&json) {
                Ok(msg) => {
                    eprintln!("msg: {:?}", msg);
                    match proto::handle_incoming(procs, msg).await {
                        Ok(Some(rsp)) => {
                            eprintln!("rsp: {:?}", rsp);
                            connection.borrow_mut().send(rsp).await?
                        }
                        Ok(None) => (),
                        Err(err) => eprintln!("message error: {:?}: {}", err, String::from_utf8(json).unwrap()),
                    }
                }
                Err(err) => eprintln!("invalid JSON: {:?}: {}", err, String::from_utf8(json).unwrap()),
            },
            _ => eprintln!("unexpected ws msg: {}", msg),
        }
        Ok(())
    }

    pub async fn run(self, procs: SharedRunningProcs) -> Result<(), Error> {
        // FIXME: Reconnect.
        self.read
            .for_each(|msg| async {
                if let Ok(msg) = msg {
                    if let Err(err) =
                        Self::handle(self.connection.clone(), procs.clone(), msg).await
                    {
                        eprintln!("error: {:?}", err);
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
    pub async fn connect(
        url: &Url,
        conn_id: Option<&str>,
        group: Option<&str>,
    ) -> Result<(SharedConnection, Handler), proto::Error> {
        let conn_id = conn_id.map_or_else(|| proto::get_default_conn_id(), |n| n.to_string());
        let group = group.map_or(proto::DEFAULT_GROUP.to_string(), |n| n.to_string());
        let info = proto::InstanceInfo::new();

        eprintln!("connecting to {}", url);
        let (ws_stream, _) = connect_async(url).await?;
        eprintln!("connected");
        let (write, read) = ws_stream.split();
        let connection = Rc::new(RefCell::new(Connection { write }));

        // Send a connect message.
        let register = proto::OutgoingMessage::Register { conn_id, group, info };
        connection.borrow_mut().send(register).await?;

        Ok((connection.clone(), Handler { read, connection }))
    }
}
