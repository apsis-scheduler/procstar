use chrono::{DateTime, Utc};
use futures_util::future::FutureExt;
use libc::pid_t;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::os::fd::RawFd;
use std::rc::Rc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use crate::environ;
use crate::err::SpecError;
use crate::err_pipe::ErrorPipe;
use crate::fd;
use crate::fd::SharedFdHandler;
use crate::res;
use crate::sig::{SignalReceiver, SignalWatcher, Signum};
use crate::spec;
use crate::spec::ProcId;
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

pub struct Proc {
    pub pid: pid_t,
    pub errors: Vec<String>,
    pub wait_info: Option<WaitInfo>,
    pub fd_handlers: FdHandlers,
    pub start_time: DateTime<Utc>,
    pub stop_time: Option<DateTime<Utc>>,
    pub start_instant: Instant,
    pub elapsed: Option<Duration>,
}

impl Proc {
    pub fn new(pid: pid_t, start_time: DateTime<Utc>, start_instant: Instant, fd_handlers: FdHandlers) -> Self {
        Self {
            pid,
            errors: Vec::new(),
            wait_info: None,
            fd_handlers,
            start_time,
            stop_time: None,
            start_instant,
            elapsed: None,
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

        let times = res::Times {
            start: self.start_time.to_rfc3339(),
            stop: self.stop_time.map(|t| t.to_rfc3339()),
            elapsed: self.elapsed.map(|d| d.as_secs_f64()),
        };

        res::ProcRes {
            errors: self.errors.clone(),
            pid: self.pid,
            times,
            status,
            rusage,
            fds,
        }
    }
}

impl std::fmt::Debug for Proc {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        f.debug_struct("Proc").field("pid", &self.pid).finish()
    }
}

type SharedProc = Rc<RefCell<Proc>>;

//------------------------------------------------------------------------------

/// Asynchronous notifications to clients when something happens.
#[derive(Clone, Debug)]
pub enum ProcNotification {
    /// Notification that a process has been created and started.
    Start(ProcId),

    /// Notification that a process has completed.
    Complete(ProcId),

    /// Notification that a process has been deleted.
    Delete(ProcId),
}

pub type ProcNotificationSender = mpsc::UnboundedSender<ProcNotification>;
pub type ProcNotificationReceiver = mpsc::UnboundedReceiver<ProcNotification>;

//------------------------------------------------------------------------------

pub struct Procs {
    /// Map from proc ID to proc object.
    procs: BTreeMap<ProcId, SharedProc>,

    /// Notification subscriptions.
    subs: Vec<ProcNotificationSender>,
}

#[derive(Clone)]
pub struct SharedProcs(Rc<RefCell<Procs>>);

impl SharedProcs {
    pub fn new() -> SharedProcs {
        SharedProcs(Rc::new(RefCell::new(Procs {
            procs: BTreeMap::new(),
            subs: Vec::new(),
        })))
    }

    // FIXME: Some of these methods are unused.

    pub fn insert(&self, proc_id: ProcId, proc: SharedProc) {
        self.0.borrow_mut().procs.insert(proc_id.clone(), proc);
        // Let subscribers know that there is a new proc.
        self.notify(ProcNotification::Start(proc_id));
    }

    pub fn len(&self) -> usize {
        self.0.borrow().procs.len()
    }

    pub fn get_proc_ids<T>(&self) -> T
    where
        T: FromIterator<ProcId>,
    {
        self.0.borrow().procs.keys().map(|s| s.clone()).collect()
    }

    pub fn get(&self, proc_id: &str) -> Option<SharedProc> {
        self.0.borrow().procs.get(proc_id).cloned()
    }

    pub fn first(&self) -> Option<(ProcId, SharedProc)> {
        self.0
            .borrow()
            .procs
            .first_key_value()
            .map(|(proc_id, proc)| (proc_id.clone(), Rc::clone(proc)))
    }

    pub fn remove(&self, proc_id: ProcId) -> Option<SharedProc> {
        let proc = self.0.borrow_mut().procs.remove(&proc_id);
        self.notify(ProcNotification::Delete(proc_id.clone()));
        proc
    }

    /// Removes and returns a proc, if it is complete.
    pub fn remove_if_complete(&self, proc_id: &ProcId) -> Result<SharedProc, Error> {
        let mut procs = self.0.borrow_mut();
        if let Some(proc) = procs.procs.get(proc_id) {
            if proc.borrow().wait_info.is_some() {
                let proc = procs.procs.remove(proc_id).unwrap();
                drop(procs);
                self.notify(ProcNotification::Delete(proc_id.clone()));
                Ok(proc)
            } else {
                Err(Error::ProcRunning(proc_id.clone()))
            }
        } else {
            Err(Error::NoProcId(proc_id.clone()))
        }
    }

    pub fn pop(&self) -> Option<(ProcId, SharedProc)> {
        self.0.borrow_mut().procs.pop_first()
    }

    pub fn to_result(&self) -> res::Res {
        self.0
            .borrow()
            .procs
            .iter()
            .map(|(proc_id, proc)| (proc_id.clone(), proc.borrow().to_result()))
            .collect::<BTreeMap<_, _>>()
    }

    pub fn subscribe(&self) -> ProcNotificationReceiver {
        let (sender, receiver) = mpsc::unbounded_channel();
        self.0.borrow_mut().subs.push(sender);
        receiver
    }

    fn notify(&self, noti: ProcNotification) {
        self.0
            .borrow()
            .subs
            .iter()
            .for_each(|s| s.send(noti.clone()).unwrap());
    }
}

async fn wait_for_proc(proc: SharedProc, mut sigchld_receiver: SignalReceiver) {
    let pid = proc.borrow().pid;

    loop {
        // Wait until the process receives SIGCHLD.
        sigchld_receiver.signal().await;

        // Check if this pid has terminated, with a nonblocking wait.
        if let Some(wait_info) = wait(pid, false) {
            // Take timestamps right away.
            let stop_time = Utc::now();
            let stop_instant = Instant::now();

            // Process terminated; update its stuff.
            let mut proc = proc.borrow_mut();
            assert!(proc.wait_info.is_none());
            proc.wait_info = Some(wait_info);
            proc.stop_time = Some(stop_time);
            proc.elapsed = Some(stop_instant.duration_since(proc.start_instant));
            break;
        }
    }
}

/// Runs a recently-forked/execed process.
async fn run_proc(proc: SharedProc, sigchld_receiver: SignalReceiver, error_pipe: ErrorPipe) {
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

/// Starts zero or more new processes.  `input` maps new proc IDs to
/// corresponding process specs.  All proc IDs must be unused.
///
/// Because this function starts tasks with `spawn_local`, it must be run within
/// a `LocalSet`.
pub fn start_procs(
    specs: &spec::Procs,
    procs: SharedProcs,
) -> Result<Vec<tokio::task::JoinHandle<()>>, SpecError> {
    // First check that proc IDs aren't already in use.
    let old_proc_ids = procs.get_proc_ids::<HashSet<_>>();
    let dup_proc_ids = specs
        .keys()
        .filter(|&p| old_proc_ids.contains(p))
        .map(|p| p.to_string())
        .collect::<Vec<_>>();
    if !dup_proc_ids.is_empty() {
        return Err(SpecError::DupId(dup_proc_ids));
    }

    let (sigchld_watcher, sigchld_receiver) =
        SignalWatcher::new(tokio::signal::unix::SignalKind::child());
    let _sigchld_task = tokio::spawn(sigchld_watcher.watch());
    let mut tasks = Vec::new();

    for (proc_id, spec) in specs.into_iter() {
        let env = environ::build(std::env::vars(), &spec.env);

        let error_pipe = ErrorPipe::new().unwrap_or_else(|err| {
            eprintln!("failed to create pipe: {}", err);
            std::process::exit(exitcode::OSFILE);
        });

        let fd_handlers = spec
            .fds
            .iter()
            .map(|(fd_str, fd_spec)| fd::make_fd_handler(fd_str.clone(), fd_spec.clone()))
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
                        error_writer.try_write(format!("failed to set up fd: {}: {}", fd, err));
                        ok_to_exec = false;
                    });
                }

                if ok_to_exec {
                    let exe = &spec.argv[0];
                    // execve() only returns with an error; on success, the program is
                    // replaced.
                    let err = execve(exe.clone(), spec.argv.clone(), env).unwrap_err();
                    error_writer.try_write(format!("execve failed: {}: {}", exe, err));
                    // FIXME: Find a way to pass the error code to the parent
                    // for inclusion in results.
                    std::process::exit(63);
                } else {
                    std::process::exit(62);
                }
            }

            Ok(child_pid) => {
                // Parent process.

                let start_time = Utc::now();
                let start_instant = Instant::now();

                // FIXME: What do we do with these tasks?  We should await them later.
                let mut fd_errs: Vec<String> = Vec::new();
                let _fd_handler_tasks = fd_handlers
                    .iter()
                    .filter_map(|(ref fd, ref fd_handler)| match fd_handler.in_parent() {
                        Ok(task) => Some(task),
                        Err(err) => {
                            fd_errs.push(format!("failed to set up fd {}: {}", fd, err));
                            None
                        }
                    })
                    .collect::<Vec<_>>();

                // Construct the record of this running proc.
                let mut proc = Proc::new(child_pid, start_time, start_instant, fd_handlers);

                // Attach any fd errors.
                proc.errors.append(&mut fd_errs);
                drop(fd_errs);

                // Register the new proc.
                let proc = Rc::new(RefCell::new(proc));
                procs.insert(proc_id.clone(), proc.clone());

                // Build the task that awaits the process.
                let fut = run_proc(proc, sigchld_receiver.clone(), error_pipe);
                // Let subscribers know when it completes.
                let fut = {
                    let procs = procs.clone();
                    let proc_id = proc_id.clone();
                    fut.inspect(move |_| procs.notify(ProcNotification::Complete(proc_id)))
                };
                // Start the task.
                tasks.push(tokio::task::spawn_local(fut));
            }

            Err(err) => panic!("failed to fork: {}", err),
        }
    }

    Ok(tasks)
}

pub async fn collect_results(procs: SharedProcs) -> res::Res {
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
    while let Some((proc_id, proc)) = procs.pop() {
        let proc = Rc::try_unwrap(proc).unwrap().into_inner();
        // Build the proc res.
        result.insert(proc_id.clone(), proc.to_result());
    }
    // Nothing should be left running.
    assert!(procs.len() == 0);

    result
}
