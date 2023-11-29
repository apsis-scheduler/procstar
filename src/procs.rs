use chrono::{DateTime, Utc};
use futures_util::future::FutureExt;
use libc::pid_t;
use log::*;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::os::fd::RawFd;
use std::rc::Rc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::watch;

use crate::environ;
use crate::err::SpecError;
use crate::err_pipe::ErrorPipe;
use crate::fd;
use crate::fd::SharedFdHandler;
use crate::procinfo::{ProcStat, ProcStatm};
use crate::res;
use crate::sig::{SignalReceiver, SignalWatcher, Signum};
use crate::spec;
use crate::spec::ProcId;
use crate::sys::{execve, fork, setsid, kill, wait, WaitInfo};

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

// FIXME: Refactor this into enum for running and completed procs.
pub struct Proc {
    pub pid: pid_t,
    pub errors: Vec<String>,
    pub wait_info: Option<WaitInfo>,
    pub proc_stat: Option<ProcStat>,
    pub fd_handlers: FdHandlers,
    pub start_time: DateTime<Utc>,
    pub stop_time: Option<DateTime<Utc>>,
    pub start_instant: Instant,
    pub elapsed: Option<Duration>,
}

impl Proc {
    pub fn new(
        pid: pid_t,
        start_time: DateTime<Utc>,
        start_instant: Instant,
        fd_handlers: FdHandlers,
    ) -> Self {
        Self {
            pid,
            errors: Vec::new(),
            wait_info: None,
            proc_stat: None,
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

    pub fn get_state(&self) -> res::State {
        if self.errors.len() > 0 {
            res::State::Error
        } else if self.wait_info.is_none() {
            res::State::Running
        } else {
            res::State::Terminated
        }
    }

    pub fn to_result(&self) -> res::ProcRes {
        let (status, rusage, proc_statm) = if let Some((_, status, rusage)) = self.wait_info {
            (
                Some(res::Status::new(status)),
                Some(res::ResourceUsage::new(&rusage)),
                // proc statm isn't available for a terminated process.
                None,
            )
        } else {
            (None, None, ProcStatm::load_or_log(self.pid))
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

        let elapsed = if let Some(elapsed) = self.elapsed {
            elapsed
        } else {
            // Compute elapsed to now.
            Instant::now().duration_since(self.start_instant)
        };
        let times = res::Times {
            start: self.start_time.to_rfc3339(),
            stop: self.stop_time.map(|t| t.to_rfc3339()),
            elapsed: elapsed.as_secs_f64(),
        };

        // Use completion proc stat on the process object, if available;
        // otherwise, snapshot current.
        let proc_stat = self
            .proc_stat
            .clone()
            .or_else(|| ProcStat::load_or_log(self.pid));

        res::ProcRes {
            state: self.get_state(),
            errors: self.errors.clone(),
            pid: self.pid,
            proc_stat,
            proc_statm,
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
pub enum Notification {
    /// Notification that a process has been created and started.
    Start(ProcId),

    /// Notification that a process has completed.
    Complete(ProcId),

    /// Notification that a process has been deleted.
    Delete(ProcId),
}

type NotificationSender = broadcast::Sender<Notification>;
type NotificationReceiver = broadcast::Receiver<Notification>;

pub struct NotificationSub {
    receiver: NotificationReceiver,
}

impl NotificationSub {
    pub async fn recv(&mut self) -> Option<Notification> {
        match self.receiver.recv().await {
            Ok(noti) => Some(noti),
            Err(RecvError::Closed) => None,
            Err(RecvError::Lagged(i)) => panic!("notification subscriber lagging: {}", i),
        }
    }
}

//------------------------------------------------------------------------------

pub struct Procs {
    /// Map from proc ID to proc object.
    procs: BTreeMap<ProcId, SharedProc>,

    /// Notification subscriptions.
    subs: NotificationSender,

    /// Shutdown notification channel.
    // FIXME: Use CancellationToken instead?
    shutdown: (
        tokio::sync::watch::Sender<bool>,
        tokio::sync::watch::Receiver<bool>,
    ),
}

#[derive(Clone)]
pub struct SharedProcs(Rc<RefCell<Procs>>);

impl SharedProcs {
    pub fn new() -> SharedProcs {
        let (sender, _receiver) = broadcast::channel(1024);
        SharedProcs(Rc::new(RefCell::new(Procs {
            procs: BTreeMap::new(),
            subs: sender,
            shutdown: watch::channel(false),
        })))
    }

    // FIXME: Some of these methods are unused.

    pub fn insert(&self, proc_id: ProcId, proc: SharedProc) {
        self.0.borrow_mut().procs.insert(proc_id.clone(), proc);
        // Let subscribers know that there is a new proc.
        self.notify(Notification::Start(proc_id));
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

    pub fn first_running(&self) -> Option<(ProcId, SharedProc)> {
        self.0
            .borrow()
            .procs
            .iter()
            .filter(|(_, proc)| proc.borrow().get_state() == res::State::Running)
            .map(|(proc_id, proc)| (proc_id.clone(), Rc::clone(proc)))
            .next()
    }

    pub fn remove(&self, proc_id: ProcId) -> Option<SharedProc> {
        let item = self.0.borrow_mut().procs.remove(&proc_id);
        if item.is_some() {
            self.notify(Notification::Delete(proc_id.clone()));
        }
        item
    }

    /// Removes and returns a proc, if it is complete.
    pub fn remove_if_complete(&self, proc_id: &ProcId) -> Result<SharedProc, Error> {
        let mut procs = self.0.borrow_mut();
        if let Some(proc) = procs.procs.get(proc_id) {
            if proc.borrow().wait_info.is_some() {
                let proc = procs.procs.remove(proc_id).unwrap();
                drop(procs);
                self.notify(Notification::Delete(proc_id.clone()));
                Ok(proc)
            } else {
                Err(Error::ProcRunning(proc_id.clone()))
            }
        } else {
            Err(Error::NoProcId(proc_id.clone()))
        }
    }

    pub fn pop(&self) -> Option<(ProcId, SharedProc)> {
        let item = self.0.borrow_mut().procs.pop_first();
        if let Some((ref proc_id, _)) = item {
            self.notify(Notification::Delete(proc_id.clone()));
        }
        item
    }

    pub fn to_result(&self) -> res::Res {
        self.0
            .borrow()
            .procs
            .iter()
            .map(|(proc_id, proc)| (proc_id.clone(), proc.borrow().to_result()))
            .collect::<BTreeMap<_, _>>()
    }

    pub fn subscribe(&self) -> NotificationSub {
        NotificationSub {
            receiver: self.0.borrow().subs.subscribe(),
        }
    }

    fn notify(&self, noti: Notification) {
        let s = self.0.borrow();
        if s.subs.receiver_count() > 0 {
            s.subs.send(noti).unwrap();
        }
    }

    /// Sends a signal to all running procs.
    pub fn send_signal(&self, signum: Signum) -> Result<(), Error> {
        let mut result = Ok(());
        self.0.borrow().procs.iter().for_each(|(_, proc)| {
            let proc = proc.borrow();
            if proc.get_state() == res::State::Running {
                let res = proc.send_signal(signum);
                if res.is_err() {
                    result = res;
                }
            }
        });
        result
    }

    /// Waits until no processes are running.
    pub async fn wait_running(&self) {
        let mut sub = self.subscribe();
        while let Some((proc_id, proc)) = self.first_running() {
            drop(proc);
            // Wait for notification that this proc has completed.
            while match sub.recv().await {
                Some(Notification::Complete(i)) | Some(Notification::Delete(i)) if i == proc_id => {
                    false
                }
                Some(_) => true,
                None => false,
            } {}
        }
    }

    /// Waits until no processes remain, i.e. all are deleted.
    pub async fn wait_empty(&self) {
        let mut sub = self.subscribe();
        while let Some((proc_id, proc)) = self.first() {
            drop(proc);
            // Wait for notification that this proc is deleted.
            while match sub.recv().await {
                Some(Notification::Delete(i)) if i == proc_id => false,
                Some(_) => true,
                None => false,
            } {}
        }
    }

    /// Requests shutdown.
    pub fn set_shutdown(&self) {
        self.0.borrow().shutdown.0.send(true).unwrap();
    }

    /// Awaits a shutdown request.
    pub async fn wait_for_shutdown(&self) {
        let mut recv = self.0.borrow().shutdown.1.clone();
        recv.changed().await.unwrap();
    }
}

async fn wait_for_proc(proc: SharedProc, mut sigchld_receiver: SignalReceiver) {
    let pid = proc.borrow().pid;

    loop {
        // Wait until the process receives SIGCHLD.
        sigchld_receiver.signal().await;

        // FIXME: HACK This won't do at all.  We need a way (pidfd?) to
        // determine that this pid has completed, without wait()ing it, so we
        // can get its /proc/pid/stat first.
        // FIXME: Unwrap.
        let proc_stat = ProcStat::load_or_log(pid);

        // Check if this pid has terminated, with a nonblocking wait.
        if let Some(wait_info) = wait(pid, false) {
            // Take timestamps right away.
            let stop_time = Utc::now();
            let stop_instant = Instant::now();

            // Process terminated; update its stuff.
            let mut proc = proc.borrow_mut();
            assert!(proc.wait_info.is_none());
            proc.wait_info = Some(wait_info);
            proc.proc_stat = proc_stat;
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
    procs: &SharedProcs,
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
            error!("failed to create pipe: {}", err);
            std::process::exit(1);
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
                        error_writer.try_write(format!("failed to set up fd {}: {}", fd, err));
                        ok_to_exec = false;
                    });
                }

                // Put the child process into a new session, to avoid
                // getting signals from the parent process group.
                if let Err(err) = setsid() {
                    error_writer.try_write(format!("setsid failed: {}", err));
                    ok_to_exec = false;
                }

                if ok_to_exec {
                    let exe = &spec.argv[0];
                    // execve() only returns with an error; on success, the program is
                    // replaced.
                    let err = execve(exe.clone(), spec.argv.clone(), env).unwrap_err();
                    error_writer.try_write(format!("execve failed: {}: {}", exe, err));
                }

                std::process::exit(63);
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
                    fut.inspect(move |_| procs.notify(Notification::Complete(proc_id)))
                };
                // Start the task.
                tasks.push(tokio::task::spawn_local(fut));
            }

            Err(err) => panic!("failed to fork: {}", err),
        }
    }

    Ok(tasks)
}

pub async fn collect_results(procs: &SharedProcs) -> res::Res {
    let mut result = res::Res::new();

    // Collect proc results by removing and waiting each running proc.
    while let Some((proc_id, proc)) = procs.pop() {
        info!("collect_results: strong_count={}", Rc::strong_count(&proc));
        let proc = Rc::try_unwrap(proc).unwrap().into_inner();
        // Build the proc res.
        result.insert(proc_id.clone(), proc.to_result());
    }
    // Nothing should be left running.
    assert!(procs.len() == 0);

    result
}
