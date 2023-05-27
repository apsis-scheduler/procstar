# Schema

The API schema corresponds loosely to the [JSON API
Specification](https://jsonapi.org).

The API endpoints are described below.  On success, an endpoint returns 2xx and
a JSON body of the form,

```json
{
  "data": {
    ...
  }
}
```

On error, an endpoint returns 4xx / 5xx and a JSON body of the form,
```json
{
  "errors": [
    {
      "status": ...,
      "detail": ...,
    }
  ]
}
```


# Process results

The result of one process is an object with the following keys:

- `errors`: An array of strings describing errors that Procstar encountered
  while invoking this process or handling its file descriptors.  In normal use,
  this array is empty.

- `pid`: The process ID.  Note that if the process has completed, the operating
  system may reuse this process ID for another process.

- `status`: If the process ran and completed, an object describing its
  completion; otherwise null.  The object has these keys:

    - `status`: The operating system exit status.

    - `exit_code`: If the process completed normally, its exit code; otherwise
      null.  Typically, this is zero if the process ran successfully.

    - `signum`: If the process ended due to a signal, the signal number which
      ended it; otherwise, null.

    - `core_dump`: True if the process ended due to a signal and produced a core
      file; else false.

    One of `exit_code` and `signum` is null.

- `rusage`: If the process ran and completed, an object describing its resource
  usage as reported by the operating system; otherwise null.  See `man 2 rusage`
  for a description of fields.  Note that `maxrss` is given in bytes and `utime`
  and `stime` are given in (fractional) seconds.

- `fds`: FIXME


# Endpoints

- `GET /procs`

    Returns the current results of all processes, keyed by process ID.  The
    format of each process result is described above.

    ```json
    {
      "procs": {
        "proc-id-1": { ... },
        "proc-id-2": { ... },
        ...
      }
    }
    ```

- `GET /procs/:id`

    Returns the current result of the process whose process ID is `:id`.  The
    format of the process result is described above.

    ```json
    {
      "proc": { ... },
    }
    ```

