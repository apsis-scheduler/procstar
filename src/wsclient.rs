use futures::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use std::cell::RefCell;
use std::rc::Rc;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use url::Url;

use crate::procs::SharedRunningProcs;
use crate::proto;

//------------------------------------------------------------------------------

pub struct Connection {
    /// The remote server URL to which we connect.
    url: Url,

    /// A unique ID which identifies this instance to the server URL.  This ID
    /// should change for each procstar process, even on the same host.
    conn_id: String,

    /// The group to which this instance belongs.  A processes may be run on
    /// a group, in which case one instance in the group is used.
    group: String,

    /// Process information about this instance.
    info: proto::InstanceInfo,

    /// The write end of the websocket connection.
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
pub type WebSocket = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

/// Handler for incoming messages on a websocket client connection.
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
                    Err(err) => eprintln!(
                        "message error: {:?}: {}",
                        err,
                        String::from_utf8(json).unwrap()
                    ),
                }
            }
            Err(err) => eprintln!(
                "invalid JSON: {:?}: {}",
                err,
                String::from_utf8(json).unwrap()
            ),
        },
        Message::Close(_) => return Err(proto::Error::Close),
        _ => eprintln!("unexpected ws msg: {:?}", msg),
    }
    Ok(())
}

/// Runs the incoming half of the websocket connection, receiving,
/// processing, and responding to incoming messages.
pub async fn run(
    mut read: WebSocket,
    connection: SharedConnection,
    procs: SharedRunningProcs,
) -> Result<(), proto::Error> {
    let url = connection.borrow().url.clone();
    loop {
        match read.next().await {
            Some(Ok(msg)) => match handle(connection.clone(), procs.clone(), msg).await {
                Ok(_) => {}
                Err(proto::Error::Close) => {
                    eprintln!("connection closed; reconnecting");
                    // FIXME: Handle error and retry instead of returning!
                    let (ws_stream, _) = connect_async(&url).await?;
                    let (new_write, new_read) = ws_stream.split();
                    connection.borrow_mut().write = new_write;
                    read = new_read;

                    let mut c = connection.borrow_mut();
                    let register = proto::OutgoingMessage::Register {
                        conn_id: c.conn_id.clone(),
                        group: c.group.clone(),
                        info: c.info.clone(),
                    };
                    c.send(register).await?;
                }
                Err(err) => eprintln!("error: {:?}", err),
            },
            Some(Err(err)) => eprintln!("msg error: {:?}", err),
            None => break,
        }
    }

    Ok(())
}

impl Connection {
    pub async fn connect(
        url: &Url,
        conn_id: Option<&str>,
        group: Option<&str>,
    ) -> Result<(SharedConnection, WebSocket), proto::Error> {
        let conn_id = conn_id.map_or_else(|| proto::get_default_conn_id(), |n| n.to_string());
        let group = group.map_or(proto::DEFAULT_GROUP.to_string(), |n| n.to_string());
        let info = proto::InstanceInfo::new();

        eprintln!("connecting to {}", url);
        let (ws_stream, _) = connect_async(url).await?;
        eprintln!("connected");
        let (write, read) = ws_stream.split();
        let connection = Rc::new(RefCell::new(Connection {
            url: url.clone(),
            conn_id: conn_id.clone(),
            group: group.clone(),
            info: info.clone(),
            write,
        }));

        // Send a connect message.
        let register = proto::OutgoingMessage::Register {
            conn_id,
            group,
            info,
        };
        connection.borrow_mut().send(register).await?;

        Ok((connection, read))
    }
}
