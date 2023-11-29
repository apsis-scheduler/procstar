extern crate exitcode;

mod argv;

use log::*;
// use procstar::fd::parse_fd;
use procstar::agent;
use procstar::http;
use procstar::procs::{collect_results, start_procs, SharedProcs};
use procstar::proto;
use procstar::res;
use procstar::shutdown::{install_signal_handler, SignalStyle};
use procstar::sig::{SIGINT, SIGQUIT, SIGTERM, SIGUSR1};
use procstar::spec;

//------------------------------------------------------------------------------

async fn maybe_run_http(args: &argv::Args, procs: &SharedProcs) {
    if args.serve {
        // Run the HTTP server until we receive a shutdown signal.
        tokio::select! {
            res = http::run_http(procs.clone()) => { res.unwrap() },
            _ = procs.wait_for_shutdown() => {},
        }
    }
}

async fn maybe_run_agent(args: &argv::Args, procs: &SharedProcs) {
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

        // Keep connecting to the agent server until timeout, or until we
        // receive a shutdown signal.
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

async fn maybe_run_until_exit(args: &argv::Args, procs: &SharedProcs) {
    if args.exit {
        // Run until no processes are running, or until we receive a shutdown
        // signal.
        tokio::select! {
            _ = procs.wait_running() => {}
            _ = procs.wait_for_shutdown() => {},
        };

        // Collect results.
        let result = collect_results(procs).await;

        if args.print {
            // Print them.
            res::print(&result).unwrap_or_else(|err| {
                error!("failed to print output: {}", err);
                std::process::exit(exitcode::OSFILE);
            });
            println!("");
        }
        if let Some(ref path) = args.output {
            // Write them to a file.
            res::dump_file(&result, path).unwrap_or_else(|err| {
                error!("failed to write output {}: {}", path, err);
                std::process::exit(exitcode::OSFILE);
            });
        };

        // Ready to shut down now.
        procs.set_shutdown();
    }
}

async fn maybe_run_until_idle(args: &argv::Args, procs: &SharedProcs) {
    if args.wait {
        // Run until no processes are left, or until we receive a shutdown
        // signal.
        tokio::select! {
            _ = procs.wait_idle() => {},
            _ = procs.wait_for_shutdown() => {},
        };

        // Ready to shut down now.
        procs.set_shutdown();
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
    let local_set = tokio::task::LocalSet::new();

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

    // Start specs given on the command line.
    //
    // We intentionally don't start any services until the input processes have
    // started, to avoid races where these procs don't appear in service
    // results.
    //
    // Even though `start_procs` is not async, we have to run it in the
    // LocalSet since it starts other tasks itself.
    let _tasks = local_set
        .run_until(async { start_procs(&input.specs, &procs) })
        .await
        .unwrap_or_else(|err| {
            error!("failed to start procs: {}", err);
            std::process::exit(exitcode::DATAERR);
        });

    // Set up global signal handlers.
    install_signal_handler(&local_set, &procs, SIGTERM, SignalStyle::TermThenKill);
    install_signal_handler(&local_set, &procs, SIGINT, SignalStyle::TermThenKill);
    install_signal_handler(&local_set, &procs, SIGQUIT, SignalStyle::Kill);
    install_signal_handler(&local_set, &procs, SIGUSR1, SignalStyle::ShutdownOnIdle);

    // Run servers and/or processes until completion, as specified on the
    // command line and other shutdown behavior.
    local_set
        .run_until(async {
            tokio::join!(
                maybe_run_http(&args, &procs),
                maybe_run_agent(&args, &procs),
                maybe_run_until_exit(&args, &procs),
                maybe_run_until_idle(&args, &procs),
            )
        })
        .await;
}
