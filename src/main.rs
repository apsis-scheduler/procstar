extern crate exitcode;

mod argv;

use futures::future::join;
use log::*;
// use procstar::fd::parse_fd;
use procstar::agent;
use procstar::http;
use procstar::procs::{
    collect_results, start_procs, wait_until_empty, wait_until_not_running, SharedProcs,
};
use procstar::proto;
use procstar::res;
use procstar::sig::SignalWatcher;
use procstar::spec;

//------------------------------------------------------------------------------

async fn maybe_run_http(args: &argv::Args, procs: SharedProcs) {
    if args.serve {
        http::run_http(procs).await.unwrap(); // FIXME: unwrap
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
        if let Err(err) = agent::run(connection, procs, &cfg).await {
            error!("websocket connection failed: {err}");
            std::process::exit(1);
        }
    }
}

async fn maybe_run_until_exit(args: &argv::Args, procs: SharedProcs) {
    if args.exit {
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
        wait_until_empty(procs).await;
    } else {
        // Run forever.
        loop {
            tokio::time::sleep(core::time::Duration::MAX).await;
        }
    }
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
    let running_procs = SharedProcs::new();
    // If specs were given on the command line, start those processes now.
    let input = if let Some(p) = args.input.as_deref() {
        spec::load_file(&p).unwrap_or_else(|err| {
            eprintln!("failed to load {}: {}", p, err);
            std::process::exit(2);
        })
    } else {
        spec::Input::new()
    };

    // Set up global signal handlers.
    let (sigterm_watcher, mut sigterm_receiver) =
        SignalWatcher::new(tokio::signal::unix::SignalKind::terminate());
    local
        .run_until(async {
            tokio::task::spawn_local(sigterm_watcher.watch());
            tokio::task::spawn_local({
                info!("received SIGTERM; terminating processes");
                let procs = running_procs.clone();
                async move {
                    loop {
                        sigterm_receiver.signal().await;
                        _ = procs.send_signal_all(
                            tokio::signal::unix::SignalKind::terminate().as_raw_value(),
                        );
                    }
                }
            });
        })
        .await;

    // Start specs given on the command line.
    //
    // We intentionally don't start any services until the input processes have
    // started, to avoid races where these procs don't appear in service
    // results.
    //
    // Even though `start_procs` is not async, we have to run it in the
    // LocalSet since it starts other tasks itself.
    let _tasks = local
        .run_until(async { start_procs(&input.specs, running_procs.clone()) })
        .await
        .unwrap_or_else(|err| {
            error!("failed to start procs: {}", err);
            std::process::exit(exitcode::DATAERR);
        });

    // Run servers and/or until completion, as specified on the command line.
    local
        .run_until(join(
            join(
                maybe_run_http(&args, running_procs.clone()),
                maybe_run_agent(&args, running_procs.clone()),
            ),
            maybe_run_until_exit(&args, running_procs.clone()),
        ))
        .await;
}
