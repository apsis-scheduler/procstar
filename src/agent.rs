use futures::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use log::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::sleep;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::{
    connect_async_tls_with_config, Connector, MaybeTlsStream, WebSocketStream,
};
use url::Url;

use crate::err::Error;
use crate::net::{get_access_token, get_tls_connector};
use crate::procinfo::ProcessInfo;
use crate::procs::{get_restricted_exe, Notification, SharedProcs};
use crate::proto;

//------------------------------------------------------------------------------

/// The read end of a split websocket.
pub type SocketReceiver = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

/// The write end of a split websocket.
pub type SocketSender = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>;

pub struct Connection {
    /// The remote server URL to which we connect.
    url: Url,
    /// Information about the connection.
    conn: proto::ConnectionInfo,
    /// Information about this process running procstar.
    proc: ProcessInfo,
}

impl Connection {
    pub fn new(url: &Url, conn_id: Option<&str>, group_id: Option<&str>) -> Self {
        let url = url.clone();
        let conn_id = conn_id.map_or_else(|| proto::get_default_conn_id(), |n| n.to_string());
        let group_id = group_id.map_or(proto::DEFAULT_GROUP.to_string(), |n| n.to_string());
        let restricted_exe = get_restricted_exe();
        let conn = proto::ConnectionInfo {
            conn_id,
            group_id,
            restricted_exe,
        };
        let proc = ProcessInfo::new_self();
        Connection { url, conn, proc }
    }
}

/// Handler for incoming messages on a websocket client connection.
async fn handle(procs: &SharedProcs, msg: Message) -> Result<Option<Message>, Error> {
    match msg {
        Message::Binary(json) => {
            let msg = serde_json::from_slice::<proto::IncomingMessage>(&json);
            if let Ok(ref msg) = msg {
                trace!("< {:?}", msg);
            } else {
                if let Ok(json) = std::str::from_utf8(json.as_slice()) {
                    error!("< {}", json);
                } else {
                    error!("< {:?}", msg);
                }
            }
            if let Some(rsp) = proto::handle_incoming(procs, msg?).await {
                // FIXME: ProcResult is too big to log.
                trace!("> {:?}", rsp);
                let json = serde_json::to_vec(&rsp)?;
                Ok(Some(Message::Binary(json)))
            } else {
                Ok(None)
            }
        }
        Message::Ping(payload) => Ok(Some(Message::Pong(payload))),
        Message::Close(_) => Err(Error::from(proto::Error::Close)),
        _ => Err(Error::from(proto::Error::WrongMessageType(format!(
            "unexpected ws msg: {:?}",
            msg
        )))),
    }
}

async fn send(sender: &mut SocketSender, msg: proto::OutgoingMessage) -> Result<(), Error> {
    // FIXME: ProcResult is too big to log.
    trace!("> {:?}", msg);
    let json = serde_json::to_vec(&msg)?;
    sender.send(Message::Binary(json)).await.unwrap();
    Ok(())
}

async fn connect(connection: &mut Connection) -> Result<(SocketSender, SocketReceiver), Error> {
    let connector = Connector::NativeTls(get_tls_connector()?);
    let (ws_stream, _) =
        connect_async_tls_with_config(&connection.url, None, false, Some(connector)).await?;
    let (mut sender, mut receiver) = ws_stream.split();

    // Send a register message.
    let register = proto::OutgoingMessage::Register {
        conn: connection.conn.clone(),
        proc: connection.proc.clone(),
        access_token: get_access_token(),
    };
    send(&mut sender, register).await?;

    // The first message we received should be Registered.
    let msg = receiver.next().await;
    match msg {
        Some(Ok(Message::Binary(json))) => {
            match serde_json::from_slice::<proto::IncomingMessage>(&json)? {
                proto::IncomingMessage::Registered => Ok((sender, receiver)),
                _msg => Err(proto::Error::UnexpectedMessage(_msg))?,
            }
        }
        Some(Ok(Message::Close(_))) => Err(proto::Error::WrongMessageType(
            "websocket closed".to_owned(),
        ))?,
        Some(Ok(_)) => Err(proto::Error::WrongMessageType(format!(
            "unexpected ws msg: {:?}",
            msg
        )))?,
        Some(Err(err)) => Err(err)?,
        None => Err(proto::Error::Close)?,
    }
}

/// Constructs an outgoing message corresponding to a notification message.
fn notification_to_message(
    procs: &SharedProcs,
    noti: Notification,
) -> Option<proto::OutgoingMessage> {
    match noti {
        Notification::Start(proc_id) | Notification::NotRunning(proc_id) => {
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
        }

        Notification::Delete(proc_id) => Some(proto::OutgoingMessage::ProcDelete { proc_id }),
    }
}

/// Background task that receives notification messages through `noti_sender`,
/// converts them to outgoing messages, and sends them via `sender`.
async fn send_notification(
    procs: &SharedProcs,
    sender: &Rc<RefCell<SocketSender>>,
    noti: Notification,
) {
    // Generate the outgoing message corresponding to the
    // notification.
    if let Some(msg) = notification_to_message(&procs, noti) {
        // Send the outgoing message.
        if let Err(err) = send(&mut sender.borrow_mut(), msg).await {
            warn!("msg send error: {:?}", err);
            // Close the websocket.
            if let Err(err) = sender.borrow_mut().close().await {
                warn!("websocket close error: {:?}", err);
            }
        }
    } else {
        // No outgoing message corresponding to this
        // notification.
    }
}

/// Connection and reconnection configuration.
#[derive(Debug)]
pub struct ConnectConfig {
    /// Initial interval after an unsuccessful connection attempt.
    pub interval_start: Duration,
    /// Exponential backoff after an unsuccessful connection attempt.
    pub interval_mult: f64,
    /// Maximum interval after an unsuccessful connection attempt.
    pub interval_max: Duration,
    /// Maximum consecutive failed connection attempts.
    pub count_max: u64,
}

impl ConnectConfig {
    pub fn new() -> Self {
        Self {
            interval_start: Duration::from_secs(1),
            interval_mult: 2.0,
            interval_max: Duration::from_secs(60),
            count_max: std::u64::MAX,
        }
    }
}

pub async fn run(
    mut connection: Connection,
    procs: SharedProcs,
    cfg: &ConnectConfig,
) -> Result<(), Error> {
    info!("agent starting: {}", connection.url);

    let mut interval = cfg.interval_start;
    let mut count = 0;
    loop {
        // (Re)connect to the service.
        info!("agent connecting: {}", connection.url);
        let (sender, mut receiver) = match connect(&mut connection).await {
            Ok(pair) => {
                info!("agent connected: {}", connection.url);
                pair
            }
            Err(err) => {
                warn!("agent connection failed: {}: {}", connection.url, err);

                count += 1;
                if cfg.count_max <= count {
                    return Err(err);
                }

                // Take a break.
                sleep(interval).await;

                // Exponential backoff in interval.
                interval = interval.mul_f64(cfg.interval_mult);
                if cfg.interval_max < interval {
                    interval = cfg.interval_max;
                }

                // Reconnect, after a moment.
                continue;
            }
        };
        // Connected.  There's now a websocket sender available.
        let sender = Rc::new(RefCell::new(sender));

        // Once successfully connected, reset the reconnect interval and count.
        interval = cfg.interval_start;
        count = 0;

        let mut sub = procs.subscribe();

        // Simultaneously wait for an incoming websocket message or a
        // notification, dispatching either.
        loop {
            tokio::select! {
                ws_msg = receiver.next() => {
                    match ws_msg {
                        Some(Ok(Message::Close(_))) => {
                            if let Err(err) = sender.borrow_mut().close().await {
                                warn!(
                                    "agent connection close error: {}: {:?}",
                                    connection.url, err
                                );
                            } else {
                                info!("agent connection closed: {}", connection.url);
                            }
                            break;
                        }
                        Some(Ok(msg)) => match handle(&procs, msg).await {
                            Ok(Some(rsp))
                                // Handling the incoming message produced a response;
                                // send it back.
                                => if let Err(err) = sender.borrow_mut().send(rsp).await {
                                    warn!("msg send error: {:?}", err);
                                    break;
                                },
                            Ok(None)
                                // Handling the message produced no response.
                                => {},
                            Err(err)
                                // Error while handling the message.
                                => {
                                    warn!("msg handle error: {:?}", err);
                                    break;
                                },
                        },
                        Some(Err(err)) => {
                            warn!("msg receive error: {:?}", err);
                            break;
                        }
                        None => {
                            warn!("msg stream end");
                            break;
                        }
                    }
                },

                // Wait for a notification to arrive on the channel.
                sub_noti = sub.recv() => {
                    match sub_noti {
                        Some(noti) => send_notification(&procs, &sender, noti).await,
                        None => {
                            info!("notification subscription closed");
                            // Do anything else?
                        }
                    }
                },
            }
        }

        // The connection is closed.  Go back and reconnect.
    }
}
