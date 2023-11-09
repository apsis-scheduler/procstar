extern crate exitcode;

mod argv;

use futures::future::join;
use log::*;
// use procstar::fd::parse_fd;
use procstar::http;
use procstar::procs::{collect_results, start_procs, SharedProcs};
use procstar::proto;
use procstar::res;
use procstar::spec;
use procstar::wsclient;

//------------------------------------------------------------------------------

async fn maybe_run_http(args: &argv::Args, running_procs: SharedProcs) {
    if args.serve {
        http::run_http(running_procs).await.unwrap(); // FIXME: unwrap
    }
}

async fn maybe_run_agent(args: &argv::Args, running_procs: SharedProcs) {
    if args.agent {
        let hostname = proto::expand_hostname(&args.agent_host).unwrap_or_else(|| {
            eprintln!("no agent server hostname; use --agent-host or set PROCSTAR_AGENT_HOST");
            std::process::exit(2);
        });
        let url = url::Url::parse(&format!("wss://{}:{}", hostname, args.agent_port)).unwrap();
        info!("agent connecting to: {}", url);
        let connection =
            wsclient::Connection::new(&url, args.conn_id.as_deref(), args.group_id.as_deref());
        let cfg = argv::get_connect_config(args);
        if let Err(err) = wsclient::run(connection, running_procs, &cfg).await {
            error!("websocket connection failed: {err}");
            std::process::exit(1);
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

    // We run tokio in single-threaded mode.
    let local = tokio::task::LocalSet::new();

    if args.serve || args.agent {
        // Start specs from the command line.  Discard the tasks.  We
        // intentionally don't start the HTTP service until the input processes
        // have started, to avoid races where these procs don't appear in HTTP
        // results.
        //
        // Even though `start_procs` is not async, we have to run it in the
        // LocalSet since it starts other tasks itself.
        local
            .run_until(async { start_procs(&input.specs, running_procs.clone()) })
            .await
            .unwrap_or_else(|err| {
                error!("failred to start procs: {}", err);
                std::process::exit(exitcode::DATAERR);
            });

        // Now run one or both servers.
        local
            .run_until(join(
                maybe_run_http(&args, running_procs.clone()),
                maybe_run_agent(&args, running_procs.clone()),
            ))
            .await;
    } else {
        // FIXME: There should be a flag for "run to completion" and "print results"?
        local
            .run_until(async move {
                // Start specs from the command line.
                let tasks =
                    start_procs(&input.specs, running_procs.clone()).unwrap_or_else(|err| {
                        error!("failed to start procs: {}", err);
                        std::process::exit(exitcode::DATAERR);
                    });
                // Wait for tasks to complete.
                for task in tasks {
                    _ = task.await.unwrap(); // FIXME: unwrap
                }
                // Collect results.
                let result = collect_results(running_procs).await;
                // Print them.
                if let Some(path) = args.output {
                    res::dump_file(&result, &path).unwrap_or_else(|err| {
                        error!("failed to write output {}: {}", path, err);
                        std::process::exit(exitcode::OSFILE);
                    });
                } else {
                    res::print(&result).unwrap_or_else(|err| {
                        error!("failed to print output: {}", err);
                        std::process::exit(exitcode::OSFILE);
                    });
                    println!("");
                }
            })
            .await;
        let ok = true; // FIXME: Determine if something went wrong.
        std::process::exit(if ok { exitcode::OK } else { 1 });
    }
}
