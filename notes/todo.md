### Cleanups

- [ ] deal with ws ping/pong
- [ ] if execve fails, return error code to parent for inclusion in result

### Features

- [x] include proc ID and group ID in results
- [x] include info about procstar instance in results: pid host user
- [ ] include rusage or similar (from /proc) in results before completion
- [ ] add starting CWD to result
- [ ] add env to result
- [x] measure start, stop, elapsed time and add to result
- [ ] specify exe explicitly
- [ ] command-line exe whitelist
- [ ] manage umask
- [ ] pdeath_sig
- [ ] signal disposition
- [ ] API for retrieving fd text (raw, UTF-8, compressed)
- [ ] API for including fd text in proc results, or not
- [ ] API for cleaning up jobs
- [ ] limit size (start? end? both?) of memory capture
- [ ] set pipe buffer sizes to max; adjust pipe read sizes
- [ ] close all fds by default?  (see syscall `close_range`)
- [ ] accept a mapping for fds in spec (optional, since order matters)
- [ ] capture fd to named (not unlinked) temp file
- [ ] make `get_result` async
- [ ] base2048 output format
- [ ] compress output

### WebSockets connection

- [ ] auth secret to connect


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


