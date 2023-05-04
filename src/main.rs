extern crate exitcode;

// Used for tests.
#[allow(unused_imports)]
#[macro_use] extern crate maplit;

use procstar::environ;
use procstar::err_pipe::new_err_pipe;
use procstar::fd::parse_fd;
use procstar::res;
use procstar::sel;
use procstar::sig;
use procstar::spec;
use procstar::sys;
use tokio::main;
use libc::pid_t;

//------------------------------------------------------------------------------

// FIXME: Elsewhere.
fn wait(block: bool) -> Option<sys::WaitInfo> {
    loop {
        match sys::wait4(-1, block) {
            Ok(Some(ti)) =>
                return Some(ti),
            Ok(None) =>
                if block {
                    panic!("wait4 empty result");
                }
                else {
                    return None;
                },
            Err(ref err)if err.kind() == std::io::ErrorKind::Interrupted =>
                // wait4 interrupted, possibly by SIGCHLD.
                if block {
                    // Keep going.
                    continue;
                }
                else {
                    // Return, as the caller might want to do something.
                    return None;
                },
            Err(err) => panic!("wait4 failed: {}", err),
        };
    }
}

//------------------------------------------------------------------------------

/// A proc we're running, or that has terminated.
struct Proc {
    pub pid: pid_t,

    /// None while the proc is running; the result of wait4() once the proc has
    /// terminated and been cleaned up.
    pub wait_info: Option<sys::WaitInfo>,
}

struct Procs {
    procs: Vec<Proc>,
    num_running: usize,
}

impl Procs {
    pub fn new() -> Self {
        Self { procs: Vec::new(), num_running: 0 }
    }

    pub fn push(&mut self, pid: pid_t) {
        self.procs.push(Proc { pid, wait_info: None });
        self.num_running += 1;
    }

    fn wait(&mut self, block: bool) {
        while self.num_running > 0 {
            if let Some(wait_info) = wait(block) {
                let pid = wait_info.0;
                let mut pid_found = false;
                for proc in &mut self.procs {
                    if proc.pid == pid {
                        assert!(proc.wait_info.replace(wait_info).is_none());
                        self.num_running -= 1;
                        pid_found = true;
                        break
                    }
                }
                assert!(pid_found, "wait returned unexpected pid: {}", pid);
            }
            else {
                assert!(! block, "blocking wait returned no pid");
                break
            }
        }
    }

    /// Waits any procs that terminated and are zombies, and stores their wait
    /// info.
    pub fn wait_any(&mut self) { self.wait(false); }

    /// Blocks and waits for all remaining procs to terminate, and stores their
    /// wait info.
    pub fn wait_all(&mut self) { self.wait(true); }

    pub fn into_iter(self) -> std::vec::IntoIter<Proc> { self.procs.into_iter() }
}

//------------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let json_path = match std::env::args().skip(1).next() {
        Some(p) => p,
        None => panic!("no file given"),  // FIXME
    };

    let input = spec::load_file(&json_path).unwrap_or_else(|err| {
        eprintln!("failed to load {}: {}", json_path, err);
        std::process::exit(exitcode::OSFILE);
    });
    eprintln!("input: {:?}", input);
    eprintln!("");

    let mut result = res::Res::new();

    // Set up the selector, which will manage events while the child runs.
    let mut select = sel::Select::new();

    // Build pipe for passing errors from child to parent.
    let (mut err_read, err_write) = new_err_pipe();
    // Read errors from the error pipe.
    select.insert_reader(&mut err_read);

    // Build the objects presenting each of the file descriptors in each proc.
    let mut fds = input.procs.iter().map(|spec| {
        spec.fds.iter().map(|(fd_str, fd_spec)| {
            // FIXME: Parse when deserializing, rather than here.
            let fd_num = parse_fd(fd_str).unwrap_or_else(|err| {
                eprintln!("failed to parse fd {}: {}", fd_str, err);
                std::process::exit(exitcode::OSERR);
            });

            procstar::fd::create_fd(fd_num, &fd_spec).unwrap_or_else(|err| {
                eprintln!("failed to create fd {}: {}", fd_str, err);
                std::process::exit(exitcode::OSERR);
            })
        }).collect::<Vec<_>>()
    }).collect::<Vec<_>>();

    let mut procs = Procs::new();
    for (spec, proc_fds) in input.procs.into_iter().zip(fds.iter_mut()) {
        let env = environ::build(std::env::vars(), &spec.env);

        // Fork the child process.
        let child_pid = sys::fork().unwrap_or_else(|err| {
            panic!("failed to fork: {}", err);
        });

        if child_pid == 0 {
            // Child process.

            // Close the read end of the error pipe.
            err_read.close().unwrap();

            let mut ok = true;
            for fd in &mut *proc_fds {
                fd.set_up_in_child().unwrap_or_else(|err| {
                    err_write.send(&format!("failed to set up fd {}: {}", fd.get_fd(), err));
                    ok = false;
                });
            }
            if !ok {
                std::process::exit(exitcode::OSERR);
            }

            let exe = &spec.argv[0];
            let err = sys::execve(exe.clone(), spec.argv.clone(), env).unwrap_err();

            // If we got here, exec failed; send the error to the parent process.
            err_write.send(&format!("exec: {}: {}", exe, err));
            std::process::exit(exitcode::OSERR);
        }

        else {
            // Parent process.  Construct the record of this running proc.
            procs.push(child_pid);
        }
    }

    // Install a SIGCHLD signal handler that sets a flag.  This way we know when
    // a proc has terminated.
    let sigchld_flag = sig::SignalFlag::new(libc::SIGCHLD);

    // Close the write end of the error pipe.
    err_write.close().unwrap();

    // Finish setting up all file descriptors for all procs.
    for proc_fds in &mut fds {
        for fd in proc_fds {
            let f = fd.get_fd();
            match (*fd).set_up_in_parent() {
                Err(err) => result.errors.push(format!("failed to set up fd {}: {}", f, err)),
                Ok(None) => (),
                Ok(Some(read)) => select.insert_reader(read),
            };
        }
    }

    // Clean up procs that might have completed already.
    procs.wait_any();
    // Now we wait for the procs to run.
    while select.any() {
        match select.select(None) {
            Ok(_) => {
                // select did something.  Keep going.
            },
            Err(ref err) if err.kind() == std::io::ErrorKind::Interrupted => {
                // select interrupted, possibly by SIGCHLD.  Keep going.
            },
            Err(err) => {
                panic!("select failed: {}", err)
            },
        };
        // If we received SIGCHLD, clean up any terminated procs.
        if sigchld_flag.get() {
            procs.wait_any();
        }
    };
    std::mem::drop(select);

    // Wait for all remaining procs to terminate and clean them up.
    procs.wait_all();

    // Collect proc results.
    result.procs = procs.into_iter()
        .zip(fds.into_iter())
        .map(|(proc, fds)| {
            let (_, status, rusage) = proc.wait_info.unwrap();

            // Build the proc res.
            let mut proc_res = res::ProcRes::new(proc.pid, status, rusage);

            // Build fd res's into it.
            for mut fd in fds {
                match fd.clean_up_in_parent() {
                    Ok(Some(fd_result)) => {
                        proc_res.fds.insert(
                            procstar::fd::get_fd_name(fd.get_fd()), fd_result);
                    }
                    Ok(None) => {
                    },
                    Err(err) => {
                        proc_res.fds.insert(
                            procstar::fd::get_fd_name(fd.get_fd()), res::FdRes::Error {});
                        result.errors.push(
                            format!("failed to clean up fd {}: {}", fd.get_fd(), err));
                    },
                };
            }

            proc_res
        }).collect();

    // Transfer errors retrieved from the error pipe buffer into results.
    result.errors.append(&mut err_read.get_errors());

    res::print(&result);
    println!("");

    std::process::exit(if result.errors.len() > 0 { 1 } else { exitcode::OK });
}

