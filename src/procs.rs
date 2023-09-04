use libc::pid_t;
use std::collections::BTreeMap;
use std::os::fd::RawFd;
use std::sync::{Arc, Mutex};

use crate::environ;
use crate::err_pipe::ErrorPipe;
use crate::fd;
use crate::fd::SharedFdHandler;
use crate::res;
use crate::sig::{SignalReceiver, SignalWatcher, Signum};
use crate::spec::{Input, ProcId};
use crate::sys::{execve, fork, kill, wait, WaitInfo};

//------------------------------------------------------------------------------

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    NoProcId(ProcId),
    ProcRunning(ProcId),
    ProcNotRunning(ProcId),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Io(err) => write!(f, "error: {}", err),
            Error::NoProcId(proc_id) => write!(f, "unknown proc ID: {}", proc_id),
            Error::ProcRunning(proc_id) => write!(f, "process running: {}", proc_id),
            Error::ProcNotRunning(proc_id) => write!(f, "process not running: {}", proc_id),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Error {
        Error::Io(err)
    }
}

//------------------------------------------------------------------------------

type FdHandlers = Vec<(RawFd, SharedFdHandler)>;

pub struct RunningProc {
    pub pid: pid_t,
    pub errors: Vec<String>,
    pub wait_info: Option<WaitInfo>,
    pub fd_handlers: FdHandlers,
}

impl RunningProc {
    pub fn new(pid: pid_t, fd_handlers: FdHandlers) -> Self {
        Self {
            pid,
            errors: Vec::new(),
            wait_info: None,
            fd_handlers,
        }
    }

    pub fn send_signal(&self, signum: Signum) -> Result<(), Error> {
        Ok(kill(self.pid, signum)?)
    }

    pub fn to_result(&self) -> res::ProcRes {
        let (status, rusage) = if let Some((_, status, rusage)) = self.wait_info {
            (
                Some(res::Status::new(status)),
                Some(res::ResourceUsage::new(&rusage)),
            )
        } else {
            (None, None)
        };

        let fds = self
            .fd_handlers
            .iter()
            .map(|(fd_num, fd_handler)| {
                let result = match fd_handler.get_result() {
                    Ok(fd_result) => fd_result,
                    Err(_err) => {
                        // result
                        //     .errors
                        //     .push(format!("failed to clean up fd {}: {}", fd.get_fd(), err));
                        // FIXME: Put the error in here.
                        res::FdRes::Error {}
                    }
                };
                (fd::get_fd_name(*fd_num), result)
            })
            .collect::<BTreeMap<_, _>>();

        res::ProcRes {
            pid: self.pid,
            errors: self.errors.clone(),
            status,
            rusage,
            fds,
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

type SharedRunningProc = Arc<Mutex<RunningProc>>;
pub type RunningProcs = BTreeMap<ProcId, SharedRunningProc>;

#[derive(Clone)]
pub struct SharedRunningProcs {
    procs: Arc<Mutex<RunningProcs>>,
}

impl SharedRunningProcs {
    pub fn new() -> SharedRunningProcs {
        SharedRunningProcs {
            procs: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    // FIXME: Some of these methods are unused.

    pub fn insert(&self, proc_id: ProcId, proc: SharedRunningProc) {
        self.procs.lock().unwrap().insert(proc_id, proc);
    }

    pub fn len(&self) -> usize {
        self.procs.lock().unwrap().len()
    }

    pub fn get(&self, proc_id: &str) -> Result<SharedRunningProc, Error> {
        if let Some(proc) = self.procs.lock().unwrap().get(proc_id) {
            Ok(proc.clone())
        } else {
            Err(Error::NoProcId(proc_id.to_string()))
        }
    }

    pub fn first(&self) -> Option<(ProcId, SharedRunningProc)> {
        self.procs
            .lock()
            .unwrap()
            .first_key_value()
            .map(|(proc_id, proc)| (proc_id.clone(), Arc::clone(proc)))
    }

    pub fn remove(&self, proc_id: ProcId) -> Option<SharedRunningProc> {
        self.procs.lock().unwrap().remove(&proc_id)
    }

    /// Removes and returns a proc, if it is complete (has wait info).
    pub fn remove_if_complete(&self, proc_id: &ProcId) -> Result<SharedRunningProc, Error> {
        let mut procs = self.procs.lock().unwrap();
        if let Some(proc) = procs.get(proc_id) {
            if proc.lock().unwrap().wait_info.is_some() {
                Ok(procs.remove(proc_id).unwrap())
            } else {
                Err(Error::ProcRunning(proc_id.clone()))
            }
        } else {
            Err(Error::NoProcId(proc_id.clone()))
        }
    }

    pub fn pop(&self) -> Option<(ProcId, SharedRunningProc)> {
        self.procs.lock().unwrap().pop_first()
    }

    pub fn to_result(&self) -> res::Res {
        self.procs
            .lock()
            .unwrap()
            .iter()
            .map(|(proc_id, proc)| (proc_id.clone(), proc.lock().unwrap().to_result()))
            .collect::<BTreeMap<_, _>>()
    }
}

async fn wait_for_proc(proc: SharedRunningProc, mut sigchld_receiver: SignalReceiver) {
    let pid = proc.lock().unwrap().pid;

    loop {
        // Wait until the process receives SIGCHLD.
        sigchld_receiver.signal().await;

        // Check if this pid has terminated, with a nonblocking wait.
        if let Some(wait_info) = wait(pid, false) {
            let mut proc = proc.lock().unwrap();
            assert!(proc.wait_info.is_none());
            // Process terminated.
            proc.wait_info = Some(wait_info);
            break;
        }
    }
}

pub async fn run_proc(
    proc: SharedRunningProc,
    sigchld_receiver: SignalReceiver,
    error_pipe: ErrorPipe,
) {
    // FIXME: Error pipe should append directly to errors, so that they are
    // available earlier.
    let error_task = {
        let proc = Arc::clone(&proc);
        tokio::task::spawn(async move {
            let mut errors = error_pipe.in_parent().await;
            proc.lock().unwrap().errors.append(&mut errors);
        })
    };

    let wait_task = tokio::task::spawn(wait_for_proc(proc, sigchld_receiver));

    _ = error_task.await;
    _ = wait_task.await;
}

//------------------------------------------------------------------------------

pub async fn start_procs(
    input: Input,
    running_procs: SharedRunningProcs,
) -> Vec<tokio::task::JoinHandle<()>> {
    let (sigchld_watcher, sigchld_receiver) =
        SignalWatcher::new(tokio::signal::unix::SignalKind::child());
    let _sigchld_task = tokio::spawn(sigchld_watcher.watch());
    let mut tasks = Vec::new();

    for (proc_id, spec) in input.procs.into_iter() {
        let env = environ::build(std::env::vars(), &spec.env);

        let error_pipe = ErrorPipe::new().unwrap_or_else(|err| {
            eprintln!("failed to create pipe: {}", err);
            std::process::exit(exitcode::OSFILE);
        });

        let fd_handlers = spec
            .fds
            .into_iter()
            .map(|(fd_str, fd_spec)| fd::make_fd_handler(fd_str, fd_spec))
            .collect::<Vec<_>>();

        // Fork the child process.
        match fork() {
            Ok(0) => {
                // In the child process.

                // Set up to write errors, if any, back to the parent.
                let error_writer = error_pipe.in_child().unwrap();
                // True if we should finally exec.
                let mut ok_to_exec = true;

                for (fd, fd_handler) in fd_handlers.into_iter() {
                    fd_handler.in_child().unwrap_or_else(|err| {
                        error_writer.try_write(format!("failed to set up fd {}: {}", fd, err));
                        ok_to_exec = false;
                    });
                }

                if ok_to_exec {
                    let exe = &spec.argv[0];
                    // execve() only returns with an error; on success, the program is
                    // replaced.
                    let err = execve(exe.clone(), spec.argv.clone(), env).unwrap_err();
                    error_writer.try_write(format!("exec: {}: {}", exe, err));
                }
                std::process::exit(exitcode::OSERR);
            }

            Ok(child_pid) => {
                // Parent process.
                // FIXME: What do we do with these tasks?  We should await them later.
                let _fd_handler_tasks = fd_handlers
                    .iter()
                    .filter_map(|(ref fd, ref fd_handler)| {
                        match fd_handler.in_parent() {
                            Ok(task) => Some(task),
                            Err(err) => {
                                // FIXME: Push this error.
                                let err = format!("failed to set up fd {}: {}", fd, err);
                                eprintln!("{}", err);
                                None
                            }
                        }
                    })
                    .collect::<Vec<_>>();

                let proc = Arc::new(Mutex::new(RunningProc::new(child_pid, fd_handlers)));

                // Start a task to handle this child.
                tasks.push(tokio::task::spawn(run_proc(
                    Arc::clone(&proc),
                    sigchld_receiver.clone(),
                    error_pipe,
                )));

                // Construct the record of this running proc.
                running_procs.insert(proc_id, proc);
            }

            Err(err) => panic!("failed to fork: {}", err),
        }
    }

    tasks
}

pub async fn collect_results(running_procs: SharedRunningProcs) -> res::Res {
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
        let proc = Arc::try_unwrap(proc).unwrap().into_inner().unwrap();
        // Build the proc res.
        result.insert(proc_id.clone(), proc.to_result());
    }
    // Nothing should be left running.
    assert!(running_procs.len() == 0);

    result
}
