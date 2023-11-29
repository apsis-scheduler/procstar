use log::*;
use std::pin::Pin;
use std::time::Duration;
use tokio::signal::unix::{signal, SignalKind};
use tokio::time::timeout;

use crate::procs::SharedProcs;
use crate::sig::{get_abbrev, Signum, SIGKILL, SIGTERM};

//------------------------------------------------------------------------------

pub enum SignalStyle {
    /// Shuts down when next all procs are deleted.
    ShutdownOnIdle,
    /// Sends SIGTERM, then sends SIGKILL, then shuts down.
    TermThenKill,
    /// Just sends SIGKILL, then shuts down.
    Kill,
}

const TERM_TIMEOUT: Duration = Duration::from_secs(60);
const KILL_TIMEOUT: Duration = Duration::from_secs(5);

pub fn install_signal_handler(
    procs: &SharedProcs,
    signum: Signum,
    signal_style: SignalStyle,
) -> Pin<Box<dyn futures::Future<Output = ()> + 'static>> {
    let name = get_abbrev(signum).unwrap();
    let kind = SignalKind::from_raw(signum);
    let mut signal_stream = signal(kind).expect(&format!("failed to create stream: {}", name));

    let procs = procs.clone();

    Box::pin(async move {
        // Wait for the signal.
        signal_stream.recv().await;
        info!("received: {}", name);

        match signal_style {
            SignalStyle::ShutdownOnIdle => {
                info!("will shut down when idle");
                procs.set_shutdown_on_idle();
            }

            SignalStyle::TermThenKill => {
                // Send SIGTERM and wait for processes to terminate.
                info!("terminating processes");
                _ = procs.send_signal(SIGTERM);

                info!("waiting for running processes");
                if timeout(TERM_TIMEOUT, procs.wait_running()).await.is_err() {
                    warn!("running processes remain; killing");
                    // Send SIGKILL to stragglers.
                    _ = procs.send_signal(SIGKILL);
                }

                // Final wait for processes.
                if timeout(KILL_TIMEOUT, procs.wait_idle()).await.is_err() {
                    warn!("undeleted processes remain");
                }

                trace!("shutting down");
                procs.set_shutdown();
            }

            SignalStyle::Kill => {
                info!("killing processes");
                _ = procs.send_signal(SIGKILL);

                // Final wait for processes.
                if timeout(KILL_TIMEOUT, procs.wait_idle()).await.is_err() {
                    warn!("undeleted processes remain");
                }

                trace!("shutting down");
                procs.set_shutdown();
            }
        }
    })
}
