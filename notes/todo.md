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
- [ ] shutdown signal handlers also shut down HTTP/ws loops
- [ ] if a connection goes away, error its process's runs after a while
      - age out connections in `procstar.agent.server`
      - notify proc and raise from `anext(results)` when this happens
      - erorr out the run
- [ ] when procstar terminates normally, send a shutdown message and error out runs
- [ ] procstar request soft shutdown, when all procs are done (or cleaned up?)
- [ ] shell command variant of procstar program
- [ ] push intermediate Procstar metadata to running runs
- [ ] Procstar web UI
- [ ] specify exe explicitly
- [ ] command-line exe whitelist
- [ ] don't (necessarily) include output text in Procstar res; get separately
- [ ] replace sigchld_receiver with pidfd-based listener that signals when a
      specific pid completes, so `wait_for_proc()` can inspect it before `wait`ing
- [ ] bad exe and other starting failures => error state in Apsis
       (this requires fixing procstar)
- [ ] procstar HTTP client tool
- [ ] HTTP endpoint describing WS agent?

### Cleanups

- [ ] use numerical fds only in specs and results (no stdin/stdout/stderr)
- [ ] if execve fails, return error code to parent for inclusion in result
- [ ] age out old connections
- [ ] should procstar ping the ws server?


### Features

- [x] include proc ID and group ID in results
- [x] include info about procstar instance in results: pid host user
- [x] include rusage or similar (from /proc) in results before completion
- [ ] add starting CWD to result
- [ ] add env to result
- [x] measure start, stop, elapsed time and add to result
- [ ] manage umask
- [ ] pdeath_sig
- [ ] signal disposition
- [ ] perf counters in results; see https://github.com/torvalds/linux/blob/master/tools/perf/design.txt
- [ ] API for retrieving fd text (raw, UTF-8, compressed)
- [ ] API for including fd text in proc results, or not
- [x] API for cleaning up jobs
- [ ] limit size (start? end? both?) of memory capture
- [ ] set pipe buffer sizes to max; adjust pipe read sizes
- [ ] close all fds by default?  (see syscall `close_range`)
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


