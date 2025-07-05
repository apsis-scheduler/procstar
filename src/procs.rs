use chrono::{DateTime, Utc};
use futures_util::future::FutureExt;
use libc::pid_t;
use log::*;
use nix::sys::eventfd::{EfdFlags, EventFd};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::os::fd::RawFd;
use std::rc::Rc;
use std::sync::RwLock;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::watch;

use crate::environ;
use crate::err::Error;
use crate::err_pipe::ErrorPipe;
use crate::fd;
use crate::fd::{FdData, SharedFdHandler};
use crate::procinfo::{ProcStat, ProcStatm};
use crate::res;
use crate::shutdown;
use crate::sig::{SignalReceiver, SignalWatcher, Signum};
use crate::spec;
use crate::spec::ProcId;
use crate::state::State;
use crate::sys::{execve, fork, kill, setsid, wait, WaitInfo};
use crate::systemd::api::{SharedSystemdClient, UnitType};
use crate::systemd::cgroup::CGroupAccounting;
use crate::systemd::manager::UnitProperty;

//------------------------------------------------------------------------------

type FdHandlers = Vec<(RawFd, SharedFdHandler)>;

type Slice = String;

// FIXME: Refactor this into enum for running, error, terminated procs.
pub struct Proc {
    pub pid: pid_t,
    pub slice: Option<Slice>,

    pub errors: Vec<String>,

    pub fd_handlers: FdHandlers,
    pub start_time: DateTime<Utc>,
    pub start_instant: Instant,

    pub wait_info: Option<WaitInfo>,
    pub proc_stat: Option<ProcStat>,
    pub cgroup_accounting: Option<CGroupAccounting>,
    pub stop_time: Option<DateTime<Utc>>,
    pub elapsed: Option<Duration>,
}

impl Proc {
    pub fn new(
        pid: pid_t,
        slice: Option<Slice>,
        start_time: DateTime<Utc>,
        start_instant: Instant,
        fd_handlers: FdHandlers,
    ) -> Self {
        Self {
            pid,
            slice,
            errors: Vec::new(),
            wait_info: None,
            proc_stat: None,
            cgroup_accounting: None,
            fd_handlers,
            start_time,
            stop_time: None,
            start_instant,
            elapsed: None,
        }
    }

    pub fn send_signal(&self, signum: Signum) -> Result<(), Error> {
        match kill(self.pid, signum) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(Error::NoProc),
            Err(err) => {
                error!("kill: {}", err.kind());
                Err(Error::from(err))
            }
        }
    }

    pub fn get_state(&self) -> State {
        if self.errors.len() > 0 {
            State::Error
        } else if self.wait_info.is_none() {
            State::Running
        } else {
            State::Terminated
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
                        Some(res::FdRes::Error {})
                    }
                };
                (fd::get_fd_name(*fd_num), result)
            })
            .collect::<BTreeMap<_, _>>();

        // FIXME: Combine these!
        let now_time = Utc::now();
        let now_instant = Instant::now();
        let elapsed = if let Some(elapsed) = self.elapsed {
            elapsed
        } else {
            // Compute elapsed to now.
            now_instant.duration_since(self.start_instant)
        };
        let times = res::Times {
            start: self.start_time.to_rfc3339(),
            now: now_time.to_rfc3339(),
            stop: self.stop_time.map(|t| t.to_rfc3339()),
            elapsed: elapsed.as_secs_f64(),
        };

        // Use termination proc stat on the process object, if available;
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
            cgroup_accounting: self.cgroup_accounting.clone(),
            times,
            status,
            rusage,
            fds,
        }
    }

    pub fn get_fd_handler(&self, fd: RawFd) -> Option<&SharedFdHandler> {
        for (fd_num, fd_handler) in self.fd_handlers.iter() {
            if *fd_num == fd {
                return Some(fd_handler);
            }
        }
        None
    }

    /// Returns data for an fd, if available, and whether it is UTF-8 text.
    pub fn get_fd_data(
        &self,
        fd: RawFd,
        start: usize,
        stop: Option<usize>,
    ) -> Result<Option<FdData>, crate::err::Error> {
        if let Some(fd_handler) = self.get_fd_handler(fd) {
            fd_handler.get_data(start, stop)
        } else {
            Err(Error::NoFd(fd))
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

    /// Notification that a process is not running, either because it terminated
    /// or because of an error.
    NotRunning(ProcId),

    /// Notification that a process has been deleted.
    Delete(ProcId),

    /// Notification of change in shutdown state.
    ShutDown(shutdown::State),
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
        tokio::sync::watch::Sender<shutdown::State>,
        tokio::sync::watch::Receiver<shutdown::State>,
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
            shutdown: watch::channel(shutdown::State::Active),
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
            .filter(|(_, proc)| proc.borrow().get_state() == State::Running)
            .map(|(proc_id, proc)| (proc_id.clone(), Rc::clone(proc)))
            .next()
    }

    /// Removes and returns a proc, if it is not running.
    pub fn remove_if_not_running(&self, proc_id: &ProcId) -> Result<SharedProc, Error> {
        let mut procs = self.0.borrow_mut();
        // Confirm we can find the proc and it's not running.
        match procs
            .procs
            .get(proc_id)
            .map(|proc| proc.borrow().get_state() != State::Running)
        {
            Some(true) => Ok(()),
            Some(false) => Err(Error::ProcRunning(proc_id.clone())),
            None => Err(Error::NoProcId(proc_id.clone())),
        }?;
        // OK, we can proceed with removing.
        let proc = procs.procs.remove(proc_id).unwrap();
        drop(procs);
        self.check_idling();
        self.notify(Notification::Delete(proc_id.clone()));
        Ok(proc)
    }

    pub fn pop(&self) -> Option<(ProcId, SharedProc)> {
        let mut procs = self.0.borrow_mut();
        let item = procs.procs.pop_first();
        drop(procs);

        if let Some((ref proc_id, _)) = item {
            self.notify(Notification::Delete(proc_id.clone()));
        }
        self.check_idling();

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

    /// Removes all procs and returns their result.
    pub fn collect_results(&self) -> res::Res {
        // Swap out all the procs.
        let procs = {
            let mut procs = BTreeMap::<ProcId, SharedProc>::new();
            std::mem::swap(&mut procs, &mut self.0.borrow_mut().procs);
            procs
        };

        // Collect results.
        let res = procs
            .iter()
            .map(|(proc_id, proc)| (proc_id.clone(), proc.borrow().to_result()))
            .collect::<BTreeMap<_, _>>();

        // Notify that all procs are being deleted.
        procs.into_keys().for_each(|proc_id| {
            self.notify(Notification::Delete(proc_id));
        });

        self.check_idling();
        res
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
    pub fn send_signal_all(&self, signum: Signum) -> Result<(), Error> {
        let mut result = Ok(());
        self.0.borrow().procs.iter().for_each(|(_, proc)| {
            let proc = proc.borrow();
            if proc.get_state() == State::Running {
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
            // Wait for notification that this proc is not running.
            while match sub.recv().await {
                Some(Notification::NotRunning(i)) | Some(Notification::Delete(i))
                    if i == proc_id =>
                {
                    false
                }
                Some(_) => true,
                None => false,
            } {}
        }
    }

    /// Waits until no processes remain, i.e. all are deleted.
    pub async fn wait_idle(&self) {
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
    pub fn set_shutdown(&self, state: shutdown::State) {
        self.0.borrow_mut().shutdown.0.send(state.clone()).unwrap();
        self.notify(Notification::ShutDown(state));
        self.check_idling();
    }

    pub fn get_shutdown(&self) -> shutdown::State {
        self.0.borrow().shutdown.1.borrow().clone()
    }

    fn check_idling(&self) {
        let proc = self.0.borrow_mut();
        if proc.procs.is_empty() && matches!(*proc.shutdown.1.borrow(), shutdown::State::Idling) {
            drop(proc);
            self.set_shutdown(shutdown::State::Done);
        }
    }

    /// Awaits a shutdown request.
    pub async fn wait_for_shutdown(&self) {
        let mut recv = self.0.borrow().shutdown.1.clone();
        while !matches!(*recv.borrow(), shutdown::State::Done) {
            recv.changed().await.unwrap()
        }
    }
}

async fn finalize_slice(
    slice: Option<&String>,
    systemd: Option<SharedSystemdClient>,
) -> Option<CGroupAccounting> {
    let systemd = systemd?;
    let slice = slice?;

    let cgroup_path = systemd
        .get_slice_cgroup_path(slice)
        .await
        .map_err(|e| error!("getting slice {slice} cgroup failed: {e}"))
        .ok()?;
    let cgroup_accounting = CGroupAccounting::load_or_log(&cgroup_path);

    debug!("stopping slice: {}", slice);
    systemd
        .stop(slice)
        .await
        .unwrap_or_else(|err| error!("stop slice failed {slice}: {err}"));

    cgroup_accounting
}

async fn wait_for_proc(
    proc: SharedProc,
    mut sigchld_receiver: SignalReceiver,
    systemd: Option<SharedSystemdClient>,
) {
    let pid = proc.borrow().pid;

    loop {
        // Wait until the process receives SIGCHLD.
        sigchld_receiver.signal().await;

        // FIXME: HACK This won't do at all.  We need a way (pidfd?) to
        // determine that this pid has terminated without calling wait(), so we
        // can get its /proc/pid/stat first.
        let proc_stat = ProcStat::load_or_log(pid);

        // Check if this pid has terminated, with a nonblocking wait.
        if let Some(wait_info) = wait(pid, false) {
            info!("proc reaped: {}", pid);
            // Take timestamps right away.
            let stop_time = Utc::now();
            let stop_instant = Instant::now();

            let mut proc = proc.borrow_mut();

            let cgroup_accounting = finalize_slice(proc.slice.as_ref(), systemd).await;

            // Process terminated; update its stuff.
            assert!(proc.wait_info.is_none());
            proc.wait_info = Some(wait_info);
            proc.proc_stat = proc_stat;
            proc.cgroup_accounting = cgroup_accounting;
            proc.stop_time = Some(stop_time);
            proc.elapsed = Some(stop_instant.duration_since(proc.start_instant));
            break;
        }
    }
}

/// Runs a recently-forked/execed process.
async fn run_proc(
    proc: SharedProc,
    sigchld_receiver: SignalReceiver,
    error_pipe: ErrorPipe,
    systemd: Option<SharedSystemdClient>,
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

    let wait_task = tokio::task::spawn_local(wait_for_proc(proc, sigchld_receiver, systemd));

    _ = error_task.await;
    _ = wait_task.await;
}

//------------------------------------------------------------------------------

/// If some, `start_procs()` only starts a process with exactly this executable.
static RESTRICTED_EXE: RwLock<Option<String>> = RwLock::new(None);

/// Sets the restricted executable.
pub fn restrict_exe(restricted_exe: &str) {
    *RESTRICTED_EXE.write().unwrap() = Some(restricted_exe.to_string());
}

/// Returns the restricted executable, if any.
pub fn get_restricted_exe() -> Option<String> {
    RESTRICTED_EXE.read().unwrap().clone()
}

/// Returns the path to the executable to exec for the process.
fn get_exe(exe: Option<String>, argv: &Vec<String>) -> String {
    // Use the explicit exe, if given, else argv[0] per convention.
    exe.unwrap_or(argv[0].clone())
}

//------------------------------------------------------------------------------

async fn set_up_slice(
    pid: u32,
    systemd_properties: spec::SystemdProperties,
    systemd: Option<SharedSystemdClient>,
) -> Result<Option<Slice>, Error> {
    let Some(systemd) = systemd else {
        return Ok(None);
    };

    let slice_properties: Vec<UnitProperty> = systemd_properties.slice.into();
    let mut scope_properties: Vec<UnitProperty> = systemd_properties.scope.into();

    let slice = systemd
        .start_transient_unit(UnitType::Slice, &slice_properties)
        .await?;

    scope_properties.push(UnitProperty::from(("PIDs", vec![pid])));
    scope_properties.push(UnitProperty::from(("Slice", &slice)));

    systemd
        .start_transient_unit(UnitType::Scope, &scope_properties)
        .await?;

    Ok(Some(slice))
}

/// Starts zero or more new processes.  `input` maps new proc IDs to
/// corresponding process specs.  All proc IDs must be unused.
///
/// Because this function starts tasks with `spawn_local`, it must be run within
/// a `LocalSet`.
pub async fn start_procs(
    specs: spec::Procs,
    procs: &SharedProcs,
    systemd: Option<SharedSystemdClient>,
) -> Result<Vec<tokio::task::JoinHandle<()>>, Error> {
    // Check that proc IDs aren't already in use.
    let old_proc_ids = procs.get_proc_ids::<HashSet<_>>();
    let dup_proc_ids = specs
        .keys()
        .filter(|&p| old_proc_ids.contains(p))
        .map(|p| p.to_string())
        .collect::<Vec<_>>();
    for proc_id in dup_proc_ids.into_iter() {
        Err(spec::Error::DuplicateProcId(proc_id))?;
    }

    spec::validate_procs_fds(&specs)?;

    // Check that we're not shutting down.
    let shutdown_state = procs.get_shutdown();
    if !matches!(shutdown_state, shutdown::State::Active) {
        Err(Error::ShuttingDown(shutdown_state))?;
    }

    let (sigchld_watcher, sigchld_receiver) =
        SignalWatcher::new(tokio::signal::unix::SignalKind::child());
    let _sigchld_task = tokio::spawn(sigchld_watcher.watch());
    let mut tasks = Vec::new();

    let mut pipes = fd::Pipes::new();

    for (proc_id, spec) in specs.into_iter() {
        let spec::Proc {
            exe,
            argv,
            env,
            fds,
            systemd_properties,
            ..
        } = spec;
        let exe = get_exe(exe, &argv);
        let env = environ::build(std::env::vars(), &env);

        let error_pipe = ErrorPipe::new().unwrap_or_else(|err| {
            error!("failed to create pipe: {}", err);
            std::process::exit(1);
        });

        let exec_ready_event = EventFd::from_value_and_flags(0, EfdFlags::EFD_CLOEXEC)
            .unwrap_or_else(|err| {
                error!("failed to create eventfd: {}", err);
                std::process::exit(1);
            });

        let fd_handlers = fds
            .into_iter()
            .map(|(fd_str, fd_spec)| fd::make_fd_handler(&proc_id, fd_str, fd_spec, &mut pipes))
            .collect::<Vec<_>>();

        // Fork the child process.
        match fork() {
            Ok(0) => {
                // In the child process.

                // Set up to write errors, if any, back to the parent.
                let error_writer = error_pipe.in_child().unwrap();
                // True if we should finally exec.
                let mut ok_to_exec = true;

                // If a restricted executable is set, make sure ours matches.
                if let Some(restricted_exe) = RESTRICTED_EXE.read().unwrap().as_ref() {
                    if exe != *restricted_exe {
                        error_writer
                            .try_write(format!("restricted executable: {}", restricted_exe));
                        ok_to_exec = false;
                    }
                }

                for (fd, fd_handler) in fd_handlers.into_iter() {
                    fd_handler.in_child().unwrap_or_else(|err| {
                        error_writer.try_write(format!("failed to set up fd {}: {}", fd, err));
                        ok_to_exec = false;
                    });
                }
                // Close remaining pipes, having dup'ed those we connect to.
                pipes.close().unwrap();

                // Put the child process into a new session, to avoid
                // getting signals from the parent process group.
                if let Err(err) = setsid() {
                    error_writer.try_write(format!("setsid failed: {}", err));
                    ok_to_exec = false;
                }

                if let Err(err) = exec_ready_event.read() {
                    error_writer.try_write(format!("read eventfd failed: {}", err));
                    ok_to_exec = false;
                }

                if ok_to_exec {
                    // execve() only returns with an error; on success, the program is
                    // replaced.
                    let err = execve(exe.to_string(), argv, env).unwrap_err();
                    error_writer.try_write(format!("execve failed: {}: {}", exe, err));
                }

                std::process::exit(63);
            }

            Ok(child_pid) => {
                // Parent process.

                let start_time = Utc::now();
                let start_instant = Instant::now();

                let slice =
                    set_up_slice(child_pid as u32, systemd_properties, systemd.clone()).await?;

                exec_ready_event.write(1_u64).unwrap();

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
                let mut proc = Proc::new(child_pid, slice, start_time, start_instant, fd_handlers);

                // Attach any fd errors.
                proc.errors.append(&mut fd_errs);
                drop(fd_errs);

                // Register the new proc.
                let proc = Rc::new(RefCell::new(proc));
                procs.insert(proc_id.clone(), proc.clone());

                // Build the task that awaits the process.
                let fut = run_proc(proc, sigchld_receiver.clone(), error_pipe, systemd.clone());
                // Let subscribers know when it terminates.
                let fut = {
                    let procs = procs.clone();
                    let proc_id = proc_id.clone();
                    fut.inspect(move |_| procs.notify(Notification::NotRunning(proc_id)))
                };
                // Start the task.
                tasks.push(tokio::task::spawn_local(fut));

                info!("proc started: {}", child_pid);
            }

            Err(err) => panic!("failed to fork: {}", err),
        }
    }

    // Both ends of all pipes should have been used.
    assert!(pipes.close().unwrap() == 0);

    Ok(tasks)
}
