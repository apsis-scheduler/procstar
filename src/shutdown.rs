use log::*;
use std::time::Duration;
use tokio::signal::unix::{signal, SignalKind};
use tokio::task::LocalSet;

use crate::procs::SharedProcs;
use crate::sig::{get_abbrev, Signum, SIGINT, SIGKILL, SIGQUIT, SIGTERM};

//------------------------------------------------------------------------------

enum SignalStyle {
    /// Sends SIGTERM, waits, then sends SIGKILL.
    TermThenKill,
    /// Just sends SIGKILL.
    Kill,
}

#[derive(Clone)]
pub enum WaitStyle {
    /// Don't wait for processes.
    None,
    /// Waits for all processes to terminate, i.e. none running.
    Termination,
    /// Waits for all processes to be deleted.
    Deletion,
}

fn send_signal(procs: &SharedProcs, signum: Signum) {
    info!("sending {} to processes", get_abbrev(signum).unwrap());
    _ = procs.send_signal_all(signum);
}

async fn wait_for_procs(procs: &SharedProcs, wait_style: WaitStyle, timeout: Duration) {
    info!("waiting for processes");
    if let Err(_) = match wait_style {
        WaitStyle::None => { Ok(()) }
        WaitStyle::Termination => {
            tokio::time::timeout(timeout, procs.wait_until_not_running()).await
        }
        WaitStyle::Deletion => tokio::time::timeout(timeout, procs.wait_until_empty()).await,
    } {
        warn!("processes running after {} s", timeout.as_secs_f64());
    }
}

async fn install_signal_handler(
    local: &LocalSet,
    procs: &SharedProcs,
    signum: Signum,
    signal_style: SignalStyle,
    wait_style: WaitStyle,
) {
    let mut signal_stream = signal(SignalKind::from_raw(signum)).expect(&format!(
        "failed to install stream for {}",
        get_abbrev(signum).unwrap()
    ));
    local
        .run_until(async {
            // Wait for the signal.
            signal_stream.recv().await;

            match signal_style {
                SignalStyle::TermThenKill => {
                    // Send SIGTERM and wait for processes to terminate.
                    send_signal(procs, SIGTERM);
                    wait_for_procs(procs, WaitStyle::Termination, Duration::from_secs(60)).await;
                    // Send SIGKILL to stragglers.
                    send_signal(procs, SIGKILL);
                }
                SignalStyle::Kill => {
                    send_signal(procs, SIGKILL);
                }
            }

            // Final wait for processes.
            wait_for_procs(procs, wait_style, Duration::from_secs(5)).await;

            procs.set_shutdown();
        })
        .await;
}

/// Install handlers for shutdown signals.
///
/// `wait_style` specifies whether to wait (with timeout) for processes to be
/// deleted before shutting down, or simply to wait for them to terminate, or
/// not to wait at all.
pub async fn install_signal_handlers(local: &LocalSet, procs: &SharedProcs, wait_style: WaitStyle) {
    install_signal_handler(
        &local,
        &procs,
        SIGTERM,
        SignalStyle::TermThenKill,
        wait_style.clone(),
    )
    .await;

    install_signal_handler(
        &local,
        procs,
        SIGINT,
        SignalStyle::TermThenKill,
        wait_style.clone(),
    )
    .await;

    install_signal_handler(&local, procs, SIGQUIT, SignalStyle::Kill, wait_style).await;
}
