extern crate exitcode;

// Used for tests.
#[allow(unused_imports)]
#[macro_use]
extern crate maplit;

use libc::pid_t;
use procstar::environ;
use procstar::err_pipe::ErrorPipe;
// use procstar::fd::parse_fd;
use procstar::res;
use procstar::sig;
use procstar::spec;
use procstar::spec::ProcId;
use procstar::sys;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::cell::RefCell;

use hyper::body::{Body as HttpBody, Bytes, Frame};

//------------------------------------------------------------------------------

// FIXME: Elsewhere.
fn wait(pid: pid_t, block: bool) -> Option<sys::WaitInfo> {
    loop {
        match sys::wait4(pid, block) {
            Ok(Some(ti)) => {
                let (wait_pid, _, _) = ti;
                assert!(wait_pid == pid);
                return Some(ti);
            },
            Ok(None) => {
                if block {
                    panic!("wait4 empty result");
                } else {
                    return None;
                }
            }
            Err(ref err) if err.kind() == std::io::ErrorKind::Interrupted =>
            // wait4 interrupted, possibly by SIGCHLD.
            {
                if block {
                    // Keep going.
                    continue;
                } else {
                    // Return, as the caller might want to do something.
                    return None;
                }
            }
            Err(err) => panic!("wait4 failed: {}", err),
        };
    }
}

//------------------------------------------------------------------------------

type ErrorsFuture = tokio::task::JoinHandle<Vec<String>>;
type WaitFuture = tokio::task::JoinHandle<sys::WaitInfo>;

struct RunningProc {
    pub pid: pid_t,
    pub errors_task: ErrorsFuture,
    pub wait_task: WaitFuture,
}

impl std::fmt::Debug for RunningProc {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        f.debug_struct("RunningProc")
            .field("pid", &self.pid)
            .finish()
    }
}

#[derive(Clone)]
struct SharedRunningProcs {
    procs: Rc<RefCell<BTreeMap<ProcId, Rc<RefCell<RunningProc>>>>>,
}

impl SharedRunningProcs {
    pub fn new() -> SharedRunningProcs {
        SharedRunningProcs {
            procs: Rc::new(RefCell::new(BTreeMap::new())),
        }
    }

    pub async fn insert(&self, proc_id: ProcId, proc: RunningProc) {
        self.procs.borrow_mut().insert(
            proc_id,
            Rc::new(RefCell::new(proc)),
        );
    }

    pub async fn pop(&self) -> Option<(ProcId, RunningProc)> {
        if let Some((proc_id, proc)) = self.procs.borrow_mut().pop_first() {
            let proc = Rc::<RefCell<RunningProc>>::try_unwrap(proc.into()).unwrap().into_inner();
            Some((proc_id, proc))
        } else {
            None
        }
    }

    pub async fn len(&self) -> usize {
        self.procs.borrow().len()
    }
}

// /// A proc we're running, or that has terminated.
// struct Proc {
//     pub pid: pid_t,

//     pub errors_future: ErrorsFuture,

//     /// None while the proc is running; the result of wait4() once the proc has
//     /// terminated and been cleaned up.
//     pub wait_info: Option<sys::WaitInfo>,
// }

// struct Procs {
//     procs: Vec<Proc>,
//     num_running: usize,
// }

// impl Procs {
//     pub fn new() -> Self {
//         Self {
//             procs: Vec::new(),
//             num_running: 0,
//         }
//     }

//     pub fn push(&mut self, pid: pid_t, errors_future: ErrorsFuture, proc_task: ProcTask) {
//         self.procs.push(Proc {
//             pid,
//             errors_future,
//             proc_task,
//             wait_info: None,
//         });
//         self.num_running += 1;
//     }

//     fn wait(&mut self, block: bool) {
//         while self.num_running > 0 {
//             if let Some(wait_info) = wait(-1, block) {
//                 let pid = wait_info.0;
//                 let mut pid_found = false;
//                 for proc in &mut self.procs {
//                     if proc.pid == pid {
//                         assert!(proc.wait_info.replace(wait_info).is_none());
//                         self.num_running -= 1;
//                         pid_found = true;
//                         break;
//                     }
//                 }
//                 assert!(pid_found, "wait returned unexpected pid: {}", pid);
//             } else {
//                 assert!(!block, "blocking wait returned no pid");
//                 break;
//             }
//         }
//     }

//     /// Waits any procs that terminated and are zombies, and stores their wait
//     /// info.
//     pub fn wait_any(&mut self) {
//         self.wait(false);
//     }

//     /// Blocks and waits for all remaining procs to terminate, and stores their
//     /// wait info.
//     pub fn wait_all(&mut self) {
//         self.wait(true);
//     }

//     pub fn into_iter(self) -> std::vec::IntoIter<Proc> {
//         self.procs.into_iter()
//     }
// }

async fn wait_for_proc(pid: pid_t, mut sigchld_receiver: sig::SignalReceiver) -> sys::WaitInfo {
    loop {
        // Wait until the process receives SIGCHLD.  
        sigchld_receiver.signal().await;

        // Check if this pid has terminated, with a nonblocking wait.
        if let Some(wait_info) = wait(pid, false) {
            // Process terminated.
            return wait_info;
        }
    }
}

//------------------------------------------------------------------------------

struct Body {
    // Our Body type is !Send and !Sync:
    data: Option<Bytes>,
}

impl From<String> for Body {
    fn from(a: String) -> Self {
        Body {
            data: Some(a.into()),
        }
    }
}

impl HttpBody for Body {
    type Data = Bytes;
    type Error = hyper::Error;

    fn poll_frame(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        std::task::Poll::Ready(self.get_mut().data.take().map(|d| Ok(Frame::data(d))))
    }
}

async fn run_http(running_procs: SharedRunningProcs) -> Result<(), Box<dyn std::error::Error>> {
    let addr: std::net::SocketAddr = ([127, 0, 0, 1], 3000).into();

    // Using a !Send request counter is fine on 1 thread...
    let counter = Rc::new(std::cell::Cell::new(0));

    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("Listening on http://{}", addr);
    loop {
        let (stream, _) = listener.accept().await?;

        // For each connection, clone the counter to use in our service...
        let cnt = counter.clone();

        let service = hyper::service::service_fn(move |_| {
            let prev = cnt.get();
            cnt.set(prev + 1);
            let value = cnt.get();
            async move { Ok::<_, hyper::Error>(hyper::Response::new(Body::from(format!("Request #{}\n", value)))) }
        });

        tokio::task::spawn_local(async move {
            if let Err(err) = hyper::server::conn::http1::Builder::new()
                .serve_connection(stream, service)
                .await
            {
                println!("Error serving connection: {:?}", err);
            }
        });
    }
}

//------------------------------------------------------------------------------

async fn start_procs(input: spec::Input, running_procs: &SharedRunningProcs) {
    let (sigchld_watcher, sigchld_receiver) =
        sig::SignalWatcher::new(tokio::signal::unix::SignalKind::child());
    let _sigchld_task = tokio::spawn(sigchld_watcher.watch());

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

                // Start a task to receive errors from the child.
                let errors_task = tokio::spawn(error_pipe.in_parent());
                // Start a task to wait for the child to complete.
                let wait_task = tokio::spawn(wait_for_proc(child_pid, sigchld_receiver.clone()));
                // Construct the record of this running proc.
                running_procs.insert(
                    proc_id,
                    RunningProc {
                        pid: child_pid,
                        errors_task,
                        wait_task,
                    }).await;
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

    // Wait for all remaining procs to terminate and clean them up.
    // procs.wait_all();

    // Collect proc results by removing and waiting each running proc.
    while let Some((proc_id, proc)) = running_procs.pop().await {
        let errors = proc.errors_task.await.unwrap(); // FIXME
        let (_, status, rusage) = proc.wait_task.await.unwrap();

        // Build the proc res.
        let proc_res = res::ProcRes::new(errors, proc.pid, status, rusage);

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
    assert!(running_procs.len().await == 0);

    result
}

async fn run(input: spec::Input, running_procs: SharedRunningProcs) -> res::Res {
    start_procs(input, &running_procs).await;
    collect_results(running_procs).await
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let json_path = match std::env::args().skip(1).next() {
        Some(p) => p,
        None => panic!("no file given"), // FIXME
    };

    let input = spec::load_file(&json_path).unwrap_or_else(|err| {
        eprintln!("failed to load {}: {}", json_path, err);
        std::process::exit(exitcode::OSFILE);
    });
    eprintln!("input: {:?}", input);
    eprintln!("");

    let running_procs = SharedRunningProcs::new();
    let result_future = run(input, running_procs.clone());
    let http_future = run_http(running_procs);

    let local = tokio::task::LocalSet::new();
    local.run_until(async move {
        let _run_task = tokio::task::spawn_local(async move {
            let result = result_future.await;
            res::print(&result);
            println!("");
        });
        http_future.await.unwrap();  // FIXME
    }).await;

    // res::print(&result);
    // println!("");

    // std::process::exit(if result.errors.len() > 0 {
    //     1
    // } else {
    //     exitcode::OK
    // });
}
