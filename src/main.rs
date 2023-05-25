extern crate exitcode;

mod argv;

// use procstar::fd::parse_fd;
use procstar::http::run_http;
use procstar::procs::{collect_results, start_procs, SharedRunningProcs};
use procstar::res;
use procstar::spec;

//------------------------------------------------------------------------------

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
    if args.serve {
        // Service mode.
        local
            .run_until(async move {
                // Start specs from the command line.  Discard the tasks.  We
                // intentionally don't start the HTTP service until the input
                // processes have started, to avoid races where these procs
                // don't appear in HTTP results.
                start_procs(input, running_procs.clone()).await;
                // Run the HTTP service.
                run_http(running_procs).await.unwrap()
            })
            .await;
    } else {
        local
            .run_until(async move {
                // Start specs from the command line.
                let tasks = start_procs(input, running_procs.clone()).await;
                // Wait for tasks to complete.
                for task in tasks {
                    _ = task.await.unwrap();  // FIXME
                }
                // Collect results.
                let result = collect_results(running_procs).await;
                // Print them.
                res::print(&result);
                println!("");
            })
            .await;
        let ok = true; // FIXME: Determine if something went wrong.
        std::process::exit(if ok { exitcode::OK } else { 1 });
    }
}
