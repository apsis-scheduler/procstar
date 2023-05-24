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
use sys::WaitInfo;

use hyper::Request;
use hyper::body::{Bytes, Frame, Incoming, Body as HttpBody};

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

struct RunningProc {
    pub pid: pid_t,
    pub errors: Vec<String>,
    pub wait_info: Option<WaitInfo>,
}

impl RunningProc {
    fn new(pid: pid_t) -> Self {
        Self {
            pid,
            errors: Vec::new(),
            wait_info: None,
        }
    }

    fn to_result(&self) -> res::ProcRes {
        let (status, rusage) = if let Some((_, status, rusage)) = self.wait_info {
            (Some(res::Status::new(status)), Some(res::ResourceUsage::new(&rusage)))
        } else {
            (None, None)
        };
        res::ProcRes {
            pid: self.pid,
            errors: self.errors.clone(),
            status,
            rusage,
            fds: BTreeMap::new(),  // FIXME
        }
    }
}

impl std::fmt::Debug for RunningProc {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        f.debug_struct("RunningProc")
            .field("pid", &self.pid)
            .finish()
    }
}

//------------------------------------------------------------------------------

type SharedRunningProc = Rc<RefCell<RunningProc>>;
type RunningProcs = BTreeMap<ProcId, SharedRunningProc>;

#[derive(Clone)]
struct SharedRunningProcs {
    procs: Rc<RefCell<RunningProcs>>,
}

impl SharedRunningProcs {
    pub fn new() -> SharedRunningProcs {
        SharedRunningProcs {
            procs: Rc::new(RefCell::new(BTreeMap::new())),
        }
    }

    pub fn insert(&self, proc_id: ProcId, proc: SharedRunningProc) {
        self.procs.borrow_mut().insert(proc_id, proc);
    }

    pub fn len(&self) -> usize {
        self.procs.borrow().len()
    }

    pub fn get(&self, proc_id: ProcId) -> Option<SharedRunningProc> {
        self.procs.borrow().get(&proc_id).cloned()
    }

    pub fn first(&self) -> Option<(ProcId, SharedRunningProc)> {
        self.procs.borrow().first_key_value().map(|(proc_id, proc)| (proc_id.clone(), Rc::clone(proc)))
    }

    pub fn remove(&self, proc_id: ProcId) -> Option<SharedRunningProc> {
        self.procs.borrow_mut().remove(&proc_id)
    }

    pub fn pop(&self) -> Option<(ProcId, SharedRunningProc)> {
        self.procs.borrow_mut().pop_first()
    }

    pub fn to_result(&self) -> res::Res {
        let procs = self.procs.borrow().iter().map(
            |(proc_id, proc)| (proc_id.clone(), proc.borrow().to_result())
        ).collect::<BTreeMap<_, _>>();
        res::Res { procs }
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

async fn wait_for_proc(proc: SharedRunningProc, mut sigchld_receiver: sig::SignalReceiver) {
    let pid = proc.borrow().pid;

    loop {
        // Wait until the process receives SIGCHLD.  
        sigchld_receiver.signal().await;

        // Check if this pid has terminated, with a nonblocking wait.
        if let Some(wait_info) = wait(pid, false) {
            let mut proc = proc.borrow_mut();
            assert!(proc.wait_info.is_none());
            // Process terminated.
            proc.wait_info = Some(wait_info);
            break;
        }
    }
}

async fn handle_proc(
    proc: SharedRunningProc,
    sigchld_receiver: sig::SignalReceiver,
    error_pipe: ErrorPipe,
) {
    // FIXME: Error pipe should append directly to errors, so that they are
    // available earlier.
    let error_task = {
        let proc = Rc::clone(&proc);
        tokio::task::spawn_local(async move {
            let mut errors = error_pipe.in_parent().await;
            proc.borrow_mut().errors.append(&mut errors);
        })
    };

    let wait_task = tokio::task::spawn_local(wait_for_proc(proc, sigchld_receiver));

    _ = error_task.await;
    _ = wait_task.await;
}

//------------------------------------------------------------------------------

struct Body {
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

    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!("Listening on http://{}", addr);
    loop {
        let (stream, _) = listener.accept().await?;

        let running_procs = running_procs.clone();
        let service = hyper::service::service_fn(move |_req: Request<Incoming> | {
            let running_procs = running_procs.clone();
            async move {
                // let body = format!("{}\n", running_procs.len());
                let res = running_procs.to_result();
                let body = serde_json::to_string(&res).unwrap();
                Ok::<_, hyper::Error>(hyper::Response::new(Body::from(body)))
            }
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

async fn start_procs(input: spec::Input, running_procs: SharedRunningProcs) -> Vec<tokio::task::JoinHandle<()>> {
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
                tasks.push(tokio::task::spawn_local(handle_proc(Rc::clone(&proc), sigchld_receiver.clone(), error_pipe)));

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
    // FIXME: Proper CLUI.
    let mut args = std::env::args().skip(1);
    let mut input = spec::Input::new();
    let mut serve = false;
    loop {
        match args.next() {
            Some(s) if s == "-s" => {
                serve = true;
            },
            Some(p) => {
                input = spec::load_file(&p).unwrap_or_else(|err| {
                    eprintln!("failed to load {}: {}", p, err);
                    std::process::exit(exitcode::OSFILE);
                });
            },
            None => {
                break;
            },
        }
    }

    let running_procs = SharedRunningProcs::new();
    let start_fut = start_procs(input, running_procs.clone());

    // let run_future = run(input, running_procs.clone());

    let local = tokio::task::LocalSet::new();
    if serve {
        let http_fut = run_http(running_procs);
        local.run_until(async move {
            // Start specs from the command line.  Discard the tasks.
            _ = tokio::task::spawn_local(start_fut);
            // Run the service.
            http_fut.await.unwrap()
        }).await;
    } else {
        local.run_until(async move {
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
        }).await;
        let ok = true;  // FIXME: Determine if something went wrong.
        std::process::exit(if ok { exitcode::OK } else { 1 });
    }
}

