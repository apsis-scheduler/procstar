extern crate exitcode;

mod argv;

use futures::future::join;
// use procstar::fd::parse_fd;
use procstar::http;
use procstar::procs::{collect_results, start_procs, SharedProcs};
use procstar::res;
use procstar::spec;
use procstar::wsclient;

//------------------------------------------------------------------------------

async fn maybe_run_http(args: &argv::Args, running_procs: SharedProcs) {
    if args.serve {
        http::run_http(running_procs).await.unwrap(); // FIXME: unwrap
    }
}

async fn maybe_run_ws(args: &argv::Args, running_procs: SharedProcs) {
    if let Some(url) = args.connect.as_deref() {
        let url = url::Url::parse(&url).unwrap(); // FIXME: unwrap
        let connection =
            wsclient::Connection::new(&url, args.name.as_deref(), args.group_id.as_deref());
        let cfg = argv::get_connect_config(args);
        if let Err(err) = wsclient::run(connection, running_procs, &cfg).await {
            eprintln!("websocket connection failed: {err}");
            std::process::exit(1);
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args = argv::parse();

    let running_procs = SharedProcs::new();
    let input = if let Some(p) = args.input.as_deref() {
        spec::load_file(&p).unwrap_or_else(|err| {
            eprintln!("failed to load {}: {}", p, err);
            std::process::exit(exitcode::OSFILE);
        })
    } else {
        spec::Input::new()
    };

    let local = tokio::task::LocalSet::new();

    if args.serve || args.connect.is_some() {
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
                eprintln!("failred to start procs: {}", err);
                std::process::exit(exitcode::DATAERR);
            });

        // Now run one or both servers.
        local
            .run_until(join(
                maybe_run_http(&args, running_procs.clone()),
                maybe_run_ws(&args, running_procs.clone()),
            ))
            .await;
    } else {
        // FIXME: There should be a flag for "run to completion" and "print results"?
        local
            .run_until(async move {
                // Start specs from the command line.
                let tasks =
                    start_procs(&input.specs, running_procs.clone()).unwrap_or_else(|err| {
                        eprintln!("failed to start procs: {}", err);
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
                        eprintln!("failed to write output {}: {}", path, err);
                        std::process::exit(exitcode::OSFILE);
                    });
                } else {
                    res::print(&result).unwrap_or_else(|err| {
                        eprintln!("failed to print output: {}", err);
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
