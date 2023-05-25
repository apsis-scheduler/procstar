extern crate exitcode;

// Used for tests.
#[allow(unused_imports)]
#[macro_use]
extern crate maplit;

use std::cell::RefCell;
use std::rc::Rc;

mod argv;

use procstar::environ;
use procstar::err_pipe::ErrorPipe;
// use procstar::fd::parse_fd;
use procstar::http::run_http;
use procstar::procs::{run_proc, RunningProc, SharedRunningProcs};
use procstar::res;
use procstar::sig;
use procstar::spec;
use procstar::sys;

//------------------------------------------------------------------------------

async fn start_procs(
    input: spec::Input,
    running_procs: SharedRunningProcs,
) -> Vec<tokio::task::JoinHandle<()>> {
    let (sigchld_watcher, sigchld_receiver) =
        sig::SignalWatcher::new(tokio::signal::unix::SignalKind::child());
    let _sigchld_task = tokio::spawn(sigchld_watcher.watch());
    let mut tasks = Vec::new();

    // // Build the objects presenting each of the file descriptors in each proc.
    // let mut fds = input
    //     .procs
    //     .iter()
    //     .map(|spec| {
    //         spec.fds
    //             .iter()
    //             .map(|(fd_str, fd_spec)| {
    //                 // FIXME: Parse when deserializing, rather than here.
    //                 let fd_num = parse_fd(fd_str).unwrap_or_else(|err| {
    //                     eprintln!("failed to parse fd {}: {}", fd_str, err);
    //                     std::process::exit(exitcode::OSERR);
    //                 });

    //                 procstar::fd::create_fd(fd_num, &fd_spec).unwrap_or_else(|err| {
    //                     eprintln!("failed to create fd {}: {}", fd_str, err);
    //                     std::process::exit(exitcode::OSERR);
    //                 })
    //             })
    //             .collect::<Vec<_>>()
    //     })
    //     .collect::<Vec<_>>();

    for (proc_id, spec) in input.procs.into_iter() {
        let env = environ::build(std::env::vars(), &spec.env);

        let error_pipe = ErrorPipe::new().unwrap_or_else(|err| {
            eprintln!("failed to create pipe: {}", err);
            std::process::exit(exitcode::OSFILE);
        });

        // Fork the child process.
        match sys::fork() {
            Ok(0) => {
                // Child process.
                let error_writer = error_pipe.in_child().unwrap();

                let /* mut */ ok = true;
                // for fd in &mut *proc_fds {
                //     fd.set_up_in_child().unwrap_or_else(|err| {
                //         error_writer.try_write(format!(
                //             "failed to set up fd {}: {}",
                //             fd.get_fd(),
                //             err
                //         ));
                //         ok = false;
                //     });
                // }

                if ok {
                    let exe = &spec.argv[0];
                    // execve() only returns with an error; on success, the program is
                    // replaced.
                    let err = sys::execve(exe.clone(), spec.argv.clone(), env).unwrap_err();
                    error_writer.try_write(format!("exec: {}: {}", exe, err));
                }
                std::process::exit(exitcode::OSERR);
            }

            Ok(child_pid) => {
                // Parent process.

                let proc = Rc::new(RefCell::new(RunningProc::new(child_pid)));

                // Start a task to handle this child.
                tasks.push(tokio::task::spawn_local(run_proc(
                    Rc::clone(&proc),
                    sigchld_receiver.clone(),
                    error_pipe,
                )));

                // Construct the record of this running proc.
                running_procs.insert(proc_id, proc);
            }

            Err(err) => panic!("failed to fork: {}", err),
        }
    }

    // // Finish setting up all file descriptors for all procs.
    // for proc_fds in &mut fds {
    //     for fd in proc_fds {
    //         let f = fd.get_fd();
    //         match (*fd).set_up_in_parent() {
    //             Err(err) => result
    //                 .errors
    //                 .push(format!("failed to set up fd {}: {}", f, err)),
    //             Ok(None) => (),
    //             Ok(Some(read)) => select.insert_reader(read),
    //         };
    //     }
    // }

    tasks
}

async fn collect_results(running_procs: SharedRunningProcs) -> res::Res {
    let mut result = res::Res::new();

    // // Clean up procs that might have completed already.
    // procs.wait_any();
    // // Now we wait for the procs to run.
    // while select.any() {
    //     match select.select(None) {
    //         Ok(_) => {
    //             // select did something.  Keep going.
    //         }
    //         Err(ref err) if err.kind() == std::io::ErrorKind::Interrupted => {
    //             // select interrupted, possibly by SIGCHLD.  Keep going.
    //         }
    //         Err(err) => {
    //             panic!("select failed: {}", err)
    //         }
    //     };
    //     // If we received SIGCHLD, clean up any terminated procs.
    //     if sigchld_flag.get() {
    //         procs.wait_any();
    //     }
    // }
    // std::mem::drop(select);

    // Collect proc results by removing and waiting each running proc.
    while let Some((proc_id, proc)) = running_procs.pop() {
        let proc = Rc::try_unwrap(proc).unwrap().into_inner();
        // Build the proc res.
        let (_, status, rusage) = proc.wait_info.unwrap();
        let proc_res = res::ProcRes::new(proc.errors, proc.pid, status, rusage);

        // // Build fd res's into it.
        // for mut fd in fds {
        //     match fd.clean_up_in_parent() {
        //         Ok(Some(fd_result)) => {
        //             proc_res
        //                 .fds
        //                 .insert(procstar::fd::get_fd_name(fd.get_fd()), fd_result);
        //         }
        //         Ok(None) => {}
        //         Err(err) => {
        //             proc_res
        //                 .fds
        //                 .insert(procstar::fd::get_fd_name(fd.get_fd()), res::FdRes::Error {});
        //             result
        //                 .errors
        //                 .push(format!("failed to clean up fd {}: {}", fd.get_fd(), err));
        //         }
        //     };
        // }

        result.procs.insert(proc_id.clone(), proc_res);
    }
    // Nothing should be left running.
    assert!(running_procs.len() == 0);

    result
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
