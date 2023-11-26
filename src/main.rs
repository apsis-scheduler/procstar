extern crate exitcode;

mod argv;

use log::*;
// use procstar::fd::parse_fd;
use procstar::agent;
use procstar::http;
use procstar::procs::{
    collect_results, start_procs, wait_until_empty, wait_until_not_running, SharedProcs,
};
use procstar::proto;
use procstar::res;
use procstar::sig::{get_abbrev, SignalWatcher, Signum, SIGINT, SIGKILL, SIGQUIT, SIGTERM};
use procstar::spec;
use std::time::Duration;
use tokio::signal::unix::SignalKind;

//------------------------------------------------------------------------------

async fn maybe_run_http(args: &argv::Args, procs: SharedProcs) {
    if args.serve {
        tokio::select! {
            res = http::run_http(procs.clone()) => { res.unwrap() },
            _ = procs.wait_for_shutdown() => {},
        }
    }
}

async fn maybe_run_agent(args: &argv::Args, procs: SharedProcs) {
    if args.agent {
        let hostname = proto::expand_hostname(&args.agent_host).unwrap_or_else(|| {
            eprintln!("no agent server hostname; use --agent-host or set PROCSTAR_AGENT_HOST");
            std::process::exit(2);
        });
        let url = url::Url::parse(&format!("wss://{}:{}", hostname, args.agent_port)).unwrap();
        info!("agent connecting to: {}", url);
        let connection =
            agent::Connection::new(&url, args.conn_id.as_deref(), args.group_id.as_deref());
        let cfg = argv::get_connect_config(args);
        tokio::select! {
            res = agent::run(connection, procs.clone(), &cfg) => {
                if let Err(err) = res {
                    error!("websocket connection failed: {err}");
                    std::process::exit(1);
                }
            },
            _ = procs.wait_for_shutdown() => {},
        }
    }
}

async fn maybe_run_until_exit(args: &argv::Args, procs: SharedProcs) {
    if args.exit {
        tokio::select! {
            _ = wait_until_not_running(procs.clone()) => {}
            _ = procs.wait_for_shutdown() => {},
        };
        wait_until_not_running(procs.clone()).await;
        // Collect results.
        let result = collect_results(procs).await;
        // Print them.
        if args.print {
            res::print(&result).unwrap_or_else(|err| {
                error!("failed to print output: {}", err);
                std::process::exit(exitcode::OSFILE);
            });
            println!("");
        }
        if let Some(ref path) = args.output {
            res::dump_file(&result, path).unwrap_or_else(|err| {
                error!("failed to write output {}: {}", path, err);
                std::process::exit(exitcode::OSFILE);
            });
        };
    } else if args.wait {
        tokio::select! {
            _ = wait_until_empty(procs.clone()) => {},
            _ = procs.wait_for_shutdown() => {},
        };
    } else {
        // Run forever.
        loop {
            tokio::time::sleep(core::time::Duration::MAX).await;
        }
    }
}

const SIGTERM_TIMEOUT: f64 = 60.0;
const SIGKILL_TIMEOUT: f64 = 5.0;

#[derive(PartialEq)]
enum ShutdownStyle {
    TermThenKill,
    Kill,
}

async fn install_shutdown_signal(
    local: &tokio::task::LocalSet,
    procs: SharedProcs,
    signum: Signum,
    style: ShutdownStyle,
) {
    let (sigterm_watcher, mut sigterm_receiver) = SignalWatcher::new(SignalKind::from_raw(signum));
    local
        .run_until(async {
            tokio::task::spawn_local(sigterm_watcher.watch());
            tokio::task::spawn_local({
                async move {
                    sigterm_receiver.signal().await;
                    info!("received {}", get_abbrev(signum).unwrap());

                    let mut do_kill = false;

                    if style == ShutdownStyle::TermThenKill {
                        info!("terminating running processes");
                        _ = procs.send_signal_all(SIGTERM);
                        info!("waiting processes");
                        if let Err(_) = tokio::time::timeout(
                            Duration::from_secs_f64(SIGTERM_TIMEOUT),
                            wait_until_not_running(procs.clone()),
                        )
                        .await
                        {
                            warn!("processes not terminated after {} s", SIGTERM_TIMEOUT);
                            do_kill = true;
                        }
                    }

                    if style == ShutdownStyle::Kill || do_kill {
                        info!("killing running processes");
                        _ = procs.send_signal_all(SIGKILL);
                        info!("waiting processes");
                        if let Err(_) = tokio::time::timeout(
                            Duration::from_secs_f64(SIGKILL_TIMEOUT),
                            wait_until_not_running(procs.clone()),
                        )
                        .await
                        {
                            warn!("processes not killed after {} s", SIGKILL_TIMEOUT);
                        }
                    }

                    procs.set_shutdown();
                }
            });
        })
        .await;
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args = argv::parse();

    // Configure logging.
    let log_level = if let Some(l) = args.log_level.clone() {
        argv::parse_log_level(&l).unwrap()
    } else {
        log::Level::Warn
    };
    stderrlog::new()
        .module(module_path!())
        .verbosity(log_level)
        .timestamp(stderrlog::Timestamp::Millisecond)
        .init()
        .unwrap();

    // We run tokio in single-threaded mode.
    let local = tokio::task::LocalSet::new();

    // Set up the collection of processes to run.
    let procs = SharedProcs::new();
    // If specs were given on the command line, start those processes now.
    let input = if let Some(p) = args.input.as_deref() {
        if p == "-" {
            spec::load_stdin()
        } else {
            spec::load_file(&p)
        }
        .unwrap_or_else(|err| {
            eprintln!("failed to load {}: {}", p, err);
            std::process::exit(2);
        })
    } else {
        spec::Input::new()
    };

    // Set up global signal handlers.
    install_shutdown_signal(&local, procs.clone(), SIGTERM, ShutdownStyle::TermThenKill).await;
    install_shutdown_signal(&local, procs.clone(), SIGINT, ShutdownStyle::TermThenKill).await;
    install_shutdown_signal(&local, procs.clone(), SIGQUIT, ShutdownStyle::Kill).await;

    // Start specs given on the command line.
    //
    // We intentionally don't start any services until the input processes have
    // started, to avoid races where these procs don't appear in service
    // results.
    //
    // Even though `start_procs` is not async, we have to run it in the
    // LocalSet since it starts other tasks itself.
    let _tasks = local
        .run_until(async { start_procs(&input.specs, procs.clone()) })
        .await
        .unwrap_or_else(|err| {
            error!("failed to start procs: {}", err);
            std::process::exit(exitcode::DATAERR);
        });

    // Run servers and/or until completion, as specified on the command line.
    local
        .run_until(async {
            tokio::join!(
                maybe_run_http(&args, procs.clone()),
                maybe_run_agent(&args, procs.clone()),
                maybe_run_until_exit(&args, procs.clone()),
            )
        })
        .await;
}
