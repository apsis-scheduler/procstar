use libc::pid_t;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use crate::err_pipe::ErrorPipe;
use crate::res;
use crate::sig::SignalReceiver;
use crate::spec::ProcId;
use crate::sys::{wait, WaitInfo};

//------------------------------------------------------------------------------

pub struct RunningProc {
    pub pid: pid_t,
    pub errors: Vec<String>,
    pub wait_info: Option<WaitInfo>,
}

impl RunningProc {
    pub fn new(pid: pid_t) -> Self {
        Self {
            pid,
            errors: Vec::new(),
            wait_info: None,
        }
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
        res::ProcRes {
            pid: self.pid,
            errors: self.errors.clone(),
            status,
            rusage,
            fds: BTreeMap::new(), // FIXME
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
pub type RunningProcs = BTreeMap<ProcId, SharedRunningProc>;

#[derive(Clone)]
pub struct SharedRunningProcs {
    procs: Rc<RefCell<RunningProcs>>,
}

impl SharedRunningProcs {
    pub fn new() -> SharedRunningProcs {
        SharedRunningProcs {
            procs: Rc::new(RefCell::new(BTreeMap::new())),
        }
    }

    // FIXME: Some of these methods are unused.

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
        self.procs
            .borrow()
            .first_key_value()
            .map(|(proc_id, proc)| (proc_id.clone(), Rc::clone(proc)))
    }

    pub fn remove(&self, proc_id: ProcId) -> Option<SharedRunningProc> {
        self.procs.borrow_mut().remove(&proc_id)
    }

    pub fn pop(&self) -> Option<(ProcId, SharedRunningProc)> {
        self.procs.borrow_mut().pop_first()
    }

    pub fn to_result(&self) -> res::Res {
        let procs = self
            .procs
            .borrow()
            .iter()
            .map(|(proc_id, proc)| (proc_id.clone(), proc.borrow().to_result()))
            .collect::<BTreeMap<_, _>>();
        res::Res { procs }
    }
}

async fn wait_for_proc(proc: SharedRunningProc, mut sigchld_receiver: SignalReceiver) {
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

pub async fn run_proc(
    proc: SharedRunningProc,
    sigchld_receiver: SignalReceiver,
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
