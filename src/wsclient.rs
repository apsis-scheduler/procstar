use futures::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::sleep;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use url::Url;

use crate::procs::{ProcNotification, ProcNotificationReceiver, SharedProcs};
use crate::proto;

// FIXME: Replace `eprintln` for errors with something more appropriate.

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

/// Constructs an outgoing message corresponding to a notification message.
fn notification_to_message(
    procs: &SharedProcs,
    noti: ProcNotification,
) -> Option<proto::OutgoingMessage> {
    match noti {
        ProcNotification::Start(proc_id) | ProcNotification::Complete(proc_id) => {
            // Look up the proc.
            if let Some(proc) = procs.get(&proc_id) {
                // Got it.  Send its result.
                let res = proc.borrow().to_result();
                Some(proto::OutgoingMessage::ProcResult { proc_id, res })
            } else {
                // The proc has disappeared since the notification was sent;
                // it must have been deleted.
                None
            }
        },

        ProcNotification::Delete(proc_id) => Some(proto::OutgoingMessage::ProcDelete { proc_id }),
    }
}

/// Background task that receives notification messages through `noti_sender`,
/// converts them to outgoing messages, and sends them via `sender`.
async fn send_notifications(
    procs: SharedProcs,
    mut noti_receiver: ProcNotificationReceiver,
    sender: Rc<RefCell<Option<SocketSender>>>,
) {
    loop {
        // Wait for a notification to arrive on the channel.
        match noti_receiver.recv().await {
            Some(noti) => {
                // Borrow the websocket sender.
                if let Some(sender) = sender.borrow_mut().as_mut() {
                    // Generate the outgoing message corresponding to the
                    // notification.
                    if let Some(msg) = notification_to_message(&procs, noti) {
                        // Send the outgoing message.
                        if let Err(err) = send(sender, msg).await {
                            eprintln!("msg send error: {:?}", err);
                            // Close the websocket.
                            if let Err(err) = sender.close().await {
                                eprintln!("websocket close error: {:?}", err);
                            }
                        }
                    } else {
                        // No outgoing message corresponding to this
                        // notification.
                    }
                } else {
                    // No current websocket sender; we are not currently
                    // connected.  Drop this notification.
                }
            }
            // End of channel.
            None => break,
        }
    }
}

pub async fn run(mut connection: Connection, procs: SharedProcs) -> Result<(), proto::Error> {
    // Create a shared websocket sender, which is shared between the
    // notification sender and the main message loop.
    let sender: Rc<RefCell<Option<SocketSender>>> = Rc::new(RefCell::new(None));

    // Subscribe to receive asynchronous notifications, such as when a process
    // completes.
    let noti_receiver = procs.subscribe();
    // Start a task that sends notifications as outgoing messages to the
    // websocket.
    let _noti_task = tokio::task::spawn_local(send_notifications(
        procs.clone(),
        noti_receiver,
        sender.clone(),
    ));

    'connect: loop {
        // (Re)connect to the service.
        let (new_sender, mut receiver) = match connect(&mut connection).await {
            Ok(pair) => pair,
            Err(err) => {
                eprintln!("error: {:?}", err);
                // Reconnect, after a moment.
                // FIXME: Is this the right policy?
                sleep(RECONNECT_INTERVAL).await;
                continue;
            }
        };
        // Connected.  There's now a websocket sender available.
        sender.replace(Some(new_sender));

        loop {
            match receiver.next().await {
                Some(Ok(msg)) => match handle(procs.clone(), msg).await {
                    Ok(Some(rsp))
                        // Handling the incoming message produced a response;
                        // send it back.
                        => if let Err(err) = send(sender.borrow_mut().as_mut().unwrap(), rsp).await {
                            eprintln!("msg send error: {:?}", err);
                            break;
                        },
                    Ok(None)
                        // Handling the message produced no response.
                        => {},
                    Err(err)
                        // Error while handling the message.
                        => {
                            eprintln!("msg handle error: {:?}", err);
                            break;
                        },
                },
                Some(Err(err)) => {
                    eprintln!("msg receive error: {:?}", err);
                    break;
                }
                None => {
                    eprintln!("msg stream end");
                    break;
                }
            }
        }

        // The connection is closed.  No sender is available.
        sender.replace(None);

        // Go back and reconnect.
        continue 'connect;
    }
}
