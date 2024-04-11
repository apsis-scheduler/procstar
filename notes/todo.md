### Worklist

- [x] deal with ws ping/pong
- [x] TLS
- [x] connect max tries
- [x] token-based security
- [x] Apsis program type
- [x] proper logging from procstar
- [x] set up docs
- [x] clean up and rename TLS env var(s)
- [x] clean up procstar procs when done
- [x] exclude PROCSTAR_WS_* from children in Procstar, not Apsis
- [x] include signal name with signum
- [x] better metadata in run
      - [x] proc stat
      - [x] procstar info
      - [x] times
      - [x] errors
      - [x] proc statm
- [x] `PROCSTAR_WS_PORT` and `PROCSTAR_WS_HOSTNAME`
- [x] rename `PROCSTAR_WS` → `PROCSTAR_AGENT`
- [x] fix async notification of `procstar.ws.proc.Process` to signal deletion
- [x] rename `wsclient` → `agent`
- [x] rename `procstar.ws` → `procstar.agent`
- [x] clean up how errors are sent to agent
- [x] global config for procstar in Apsis
- [x] if group is unknown or has no connections, wait a while
- [x] reconnect to run on Apsis restart
- [x] add proc_id to metadata
- [x] shutdown signal handlers also shut down HTTP/ws loops
- [x] procstar request soft shutdown, when all procs are done (or cleaned up?)
- [x] procstar agents view
  - [x] track time, heartbeat, open state in connection object
  - [x] show time in web UI
  - [x] checkbox to show/hide closed connections
  - [x] auto-refresh agents view
- [x] specify exe explicitly
- [x] command-line exe whitelist
- [x] shell command variant of procstar program
- [x] implement procstar program signal
- [x] on agent reconnection, update results
- [x] intermediate meta updates to running program
- [x] intermediate output updates to running program
- [x] intermediate cleanups
    - merge `__starting_tasks` and `__running_tasks` into `__run_tasks`
    - use `TaskGroup` for `__waiting_tasks`
- [x] proxy old `Program` to `IncrementalProgram` and retire the old logic
- [x] merge feature/procstar-ws-incremental and get result updates under control
- [x] bad exe and other starting failures => error state in Apsis
- [x] push intermediate Procstar metadata to running runs
- [x] Procstar conda package
- [ ] if a connection goes away, error its process's runs after a while
      - age out connections in `procstar.agent.server`
      - notify proc and raise from `anext(results)` when this happens
      - erorr out the run
- [ ] evaluate & clean up FIXMEs
    - [ ] procstar Rust
    - [ ] procstar Python
    - [x] Apsis procstar agent
- [ ] manage outputs better, especially when they're large
    - [x] don't include captured results in output
    - [x] HTTP API for requesting full output
    - [ ] HTTP API for requesting part output
    - [ ] WebSocket API for requesting full output
    - [ ] WebSocket API for requesting part output
- [ ] incremental output in Apsis Procstar program
- [ ] output compression in procstar
- [ ] live-update run log in web UI: either cram this into `RunStore.query_live()`, or create some other protocol?
- [ ] auth in procstar HTTP service
- [ ] HTTPS in procstar HTTP service
- [ ] procstar HTTP client tool
- [ ] HTTP endpoint describing WS agent?
- [ ] clean up `Run.STATE`
- [ ] replace sigchld_receiver with pidfd-based listener that signals when a
      specific pid completes, so `wait_for_proc()` can inspect it before
      `wait`ing; NOTE: requires newer kernel than RHEL7/CentOS7
- [ ] manage file descriptor inheritance flag


### Cleanups

- [ ] in http.rs, handers should return `Rsp`
- [ ] use numerical fds only in specs and results (no stdin/stdout/stderr)
- [ ] if execve fails, return error code to parent for inclusion in result


### Features

- [ ] API for retrieving fd text (raw, UTF-8, compressed)
- [ ] API for including fd text in proc results, or not
- [ ] limit size (start? end? both?) of memory capture
- [x] include proc ID and group ID in results
- [x] include info about procstar instance in results: pid host user
- [x] include rusage or similar (from /proc) in results before completion
- [ ] close all fds by default?  (see syscall `close_range`)
- [ ] add starting CWD to result
- [ ] add env to result
- [x] measure start, stop, elapsed time and add to result
- [ ] manage umask
- [ ] pdeath_sig
- [ ] signal disposition
- [ ] perf counters in results; see https://github.com/torvalds/linux/blob/master/tools/perf/design.txt
- [x] API for cleaning up jobs
- [ ] set pipe buffer sizes to max; adjust pipe read sizes
- [ ] accept a mapping for fds in spec (optional, since order matters)
- [ ] capture fd to named (not unlinked) temp file
- [ ] make `get_result` async
- [ ] base2048 output format
- [ ] compress output


# Integration tests


# Failure handling

- What happens when one proc starts but another doesn't?
- What happens when a fd can't be set up?
  - In main?
  - In the child?
  - In the parent?


# Fd handling

Setting up arbitrary fds is tricky because `open()` will use the next available
fd.  One way around this is to dup each freshly-opened fd to fd number above any
available fd.


