use futures::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use log::*;
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
use crate::proto::{ConnectionInfo, IncomingMessage, OutgoingMessage};
use crate::shutdown;

//------------------------------------------------------------------------------

/// The read end of a split websocket.
pub type SocketReceiver = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

/// The write end of a split websocket.
pub type SocketSender = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>;

pub struct Connection {
    /// The remote server URL to which we connect.
    url: Url,
    /// Information about the connection.
    conn: ConnectionInfo,
    /// Information about this process running procstar.
    proc: ProcessInfo,
}

impl Connection {
    pub fn new(url: &Url, conn_id: Option<&str>, group_id: Option<&str>) -> Self {
        let url = url.clone();
        let conn_id = conn_id.map_or_else(|| proto::get_default_conn_id(), |n| n.to_string());
        let group_id = group_id.map_or(proto::DEFAULT_GROUP.to_string(), |n| n.to_string());
        let restricted_exe = get_restricted_exe();
        let conn = ConnectionInfo {
            conn_id,
            group_id,
            restricted_exe,
        };
        let proc = ProcessInfo::new_self();
        Connection { url, conn, proc }
    }
}

fn deserialize(data: &Vec<u8>) -> Result<IncomingMessage, Error> {
    Ok(rmp_serde::decode::from_slice::<IncomingMessage>(data)?)
}

fn serialize(msg: &OutgoingMessage) -> Result<Vec<u8>, Error> {
    Ok(rmp_serde::to_vec_named(msg)?)
}

/// Handler for incoming messages on a websocket client connection.
async fn handle(procs: &SharedProcs, msg: Message) -> Result<Option<Message>, Error> {
    match msg {
        Message::Binary(ref data) => {
            match deserialize(data) {
                Ok(msg) => {
                    trace!("< {:?}", msg);
                    if let Some(rsp) = proto::handle_incoming(procs, msg).await {
                        // FIXME: ProcResult is too big to log.
                        trace!("> {:?}", rsp);
                        Ok(Some(Message::Binary(serialize(&rsp)?)))
                    } else {
                        Ok(None)
                    }
                }
                Err(ref err) => {
                    error!("< {}: {:?}", err, msg);
                    Ok(None)
                }
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

async fn send(sender: &mut SocketSender, msg: OutgoingMessage) -> Result<(), Error> {
    // FIXME: ProcResult is too big to log AND TO SEND!
    trace!("> {:?}", msg);
    sender.send(Message::Binary(serialize(&msg)?)).await?;
    Ok(())
}

async fn connect(
    connection: &mut Connection,
    shutdown_state: shutdown::State,
) -> Result<(SocketSender, SocketReceiver), Error> {
    let connector = Connector::NativeTls(get_tls_connector()?);
    let (ws_stream, _) =
        connect_async_tls_with_config(&connection.url, None, false, Some(connector)).await?;
    let (mut sender, mut receiver) = ws_stream.split();

    // Send a register message.
    let register = OutgoingMessage::Register {
        conn: connection.conn.clone(),
        proc: connection.proc.clone(),
        access_token: get_access_token(),
        shutdown_state,
    };
    send(&mut sender, register).await?;

    // The first message we received should be Registered.
    let msg = receiver.next().await;
    match msg {
        Some(Ok(Message::Binary(ref data))) => match deserialize(data)? {
            IncomingMessage::Registered => Ok((sender, receiver)),
            _msg => Err(proto::Error::UnexpectedMessage(_msg))?,
        },
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
fn notification_to_message(procs: &SharedProcs, noti: Notification) -> Option<OutgoingMessage> {
    match noti {
        Notification::Start(proc_id) | Notification::NotRunning(proc_id) => {
            // Look up the proc.
            if let Some(proc) = procs.get(&proc_id) {
                // Got it.  Send its result.
                let res = proc.borrow().to_result();
                Some(OutgoingMessage::ProcResult { proc_id, res })
            } else {
                // The proc has disappeared since the notification was sent;
                // it must have been deleted.
                None
            }
        }

        Notification::Delete(proc_id) => Some(OutgoingMessage::ProcDelete { proc_id }),

        Notification::ShutDown => Some(OutgoingMessage::Unregister {}),
    }
}

async fn send_message(sender: &mut SocketSender, msg: OutgoingMessage) {
    if let Err(err) = send(sender, msg).await {
        warn!("msg send error: {:?}", err);
        // Close the websocket.
        if let Err(err) = sender.close().await {
            warn!("websocket close error: {:?}", err);
        }
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

fn get_state(procs: &SharedProcs) -> AgentState {
    if procs.is_shutdown() {
        AgentState::Shutdown
    } else if procs.is_shutdown_on_idle() {
        AgentState::Closed
    } else {
        AgentState::Open
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
    let mut done = false;
    while !done {
        // (Re)connect to the service.
        info!("agent connecting: {}", connection.url);
        let (mut sender, mut receiver) = match connect(&mut connection, get_state(&procs)).await {
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

        // Once successfully connected, reset the reconnect interval and count.
        interval = cfg.interval_start;
        count = 0;

        let mut sub = procs.subscribe();

        // Simultaneously wait for an incoming websocket message or a
        // notification, dispatching either.  Also watch for shutdown.
        while !done {
            tokio::select! {
                ws_msg = receiver.next() => {
                    match ws_msg {
                        Some(Ok(Message::Close(_))) => {
                            if let Err(err) = sender.close().await {
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
                                => if let Err(err) = sender.send(rsp).await {
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
                        Some(noti) => {
                            if let Notification::ShutDown = noti { done = true };
                            // Generate the outgoing message corresponding to
                            // the notification.
                            if let Some(msg) = notification_to_message(&procs, noti) {
                                send_message(&mut sender, msg).await;
                            } else {
                                // No outgoing message corresponding to this
                                // notification.
                            }
                        },
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

    Ok(())
}
