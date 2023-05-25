extern crate exitcode;

// Used for tests.
#[allow(unused_imports)]
#[macro_use]
extern crate maplit;

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
    let start_fut = start_procs(input, running_procs.clone());

    // let run_future = run(input, running_procs.clone());

    let local = tokio::task::LocalSet::new();
    if args.serve {
        let http_fut = run_http(running_procs);
        local
            .run_until(async move {
                // Start specs from the command line.  Discard the tasks.
                _ = tokio::task::spawn_local(start_fut);
                // Run the service.
                http_fut.await.unwrap()
            })
            .await;
    } else {
        local
            .run_until(async move {
                // Start specs from the command line.
                let tasks = start_fut.await;
                // Wait for tasks to complete.
                for task in tasks {
                    _ = task.await.unwrap();
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
