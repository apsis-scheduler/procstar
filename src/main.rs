extern crate exitcode;

mod argv;

use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;

// use procstar::fd::parse_fd;
use procstar::http;
use procstar::procs::{collect_results, start_procs, SharedRunningProcs};
use procstar::res;
use procstar::spec;
use procstar::wsclient;

//------------------------------------------------------------------------------

async fn run_http(running_procs: SharedRunningProcs) {
    http::run_http(running_procs).await.unwrap(); // FIXME: unwrap
}

async fn run_ws(url: String, running_procs: SharedRunningProcs) {
    let url = url::Url::parse(&url).unwrap(); // FIXME: unwrap
    let (_connection, handler) = wsclient::Connection::connect(&url).await.unwrap(); // FIXME: unwrap
    handler.run(running_procs.clone()).await.unwrap();  // FIXME: unwrap
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args = argv::parse();

    let running_procs = SharedRunningProcs::new();
    let input = if let Some(p) = args.input {
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
        // intentionally don't start the HTTP service until the input
        // processes have started, to avoid races where these procs
        // don't appear in HTTP results.
        local.run_until(start_procs(input, running_procs.clone())).await;

        let mut futs = FuturesUnordered::<_>::new();

        if args.serve {
            // Run the HTTP service.
            futs.push(futures::future::Either::Right(run_http(running_procs.clone())));
        }

        if let Some(url) = args.connect {
            futs.push(futures::future::Either::Left(run_ws(url, running_procs.clone())));
        }

        local.run_until(async move {
            while futs.next().await.is_some() {
            }
        }).await;
    } else {
        local
            .run_until(async move {
                // Start specs from the command line.
                let tasks = start_procs(input, running_procs.clone()).await;
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
