use futures::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::sleep;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use url::Url;

use crate::procs::SharedProcs;
use crate::proto;

//------------------------------------------------------------------------------

/// Wait time before reconnection attempts.
const RECONNECT_INTERVAL: Duration = Duration::new(5, 0);

/// The read end of a split websocket.
pub type SocketReceiver = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

/// The write end of a split websocket.
pub type SocketSender = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>;

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
}

impl Connection {
    // FIXME: Is it in bad taste to return a Rc<RefCell> from new()?
    pub fn new(url: &Url, conn_id: Option<&str>, group: Option<&str>) -> Self {
        let url = url.clone();
        let conn_id = conn_id.map_or_else(|| proto::get_default_conn_id(), |n| n.to_string());
        let group = group.map_or(proto::DEFAULT_GROUP.to_string(), |n| n.to_string());
        let info = proto::InstanceInfo::new();
        Connection {
            url,
            conn_id,
            group,
            info,
        }
    }
}

/// Handler for incoming messages on a websocket client connection.
async fn handle(
    procs: SharedProcs,
    msg: Message,
) -> Result<Option<proto::OutgoingMessage>, proto::Error> {
    match msg {
        Message::Binary(json) => {
            let msg = serde_json::from_slice::<proto::IncomingMessage>(&json)?;
            eprintln!("msg: {:?}", msg);
            if let Some(rsp) = proto::handle_incoming(procs, msg).await? {
                eprintln!("rsp: {:?}", rsp);
                Ok(Some(rsp))
            } else {
                Ok(None)
            }
        }
        Message::Close(_) => Err(proto::Error::Close),
        _ => Err(proto::Error::WrongMessageType(format!(
            "unexpected ws msg: {:?}",
            msg
        ))),
    }
}

async fn send(sender: &mut SocketSender, msg: proto::OutgoingMessage) -> Result<(), proto::Error> {
    let json = serde_json::to_vec(&msg)?;
    sender.send(Message::Binary(json)).await?;
    Ok(())
}

async fn connect(
    connection: &mut Connection,
) -> Result<(SocketSender, SocketReceiver), proto::Error> {
    eprintln!("connecting to {}", connection.url);
    let (ws_stream, _) = connect_async(&connection.url).await?;
    eprintln!("connected");
    let (mut sender, receiver) = ws_stream.split();

    // Send a register message.
    let register = proto::OutgoingMessage::Register {
        conn_id: connection.conn_id.clone(),
        group: connection.group.clone(),
        info: connection.info.clone(),
    };
    send(&mut sender, register).await?;

    Ok((sender, receiver))
}

pub async fn run(mut connection: Connection, procs: SharedProcs) -> Result<(), proto::Error> {
    'connect: loop {
        // (Re)connect to the service.
        let (mut sender, mut receiver) = match connect(&mut connection).await {
            Ok(pair) => pair,
            Err(err) => {
                eprintln!("error: {:?}", err);
                // Reconnect, after a moment.
                // FIXME: Is this the right policy?
                sleep(RECONNECT_INTERVAL).await;
                continue;
            }
        };

        loop {
            match receiver.next().await {
                Some(Ok(msg)) => match handle(procs.clone(), msg).await {
                    Ok(Some(rsp))
                        // Handling the incoming message procuced a response;
                        // send it back.
                        => send(&mut sender, rsp).await?,
                    Ok(None)
                        // Handling the message produced no response.
                        => {},
                    Err(err)
                        // Error while handling the message.
                        => {
                            eprintln!("msg handle error: {:?}", err);
                            continue 'connect;
                        },
                },
                Some(Err(err)) => eprintln!("msg receive error: {:?}", err),
                None => {
                    eprintln!("msg stream end");
                    continue 'connect;
                }
            }
        }
    }
}
