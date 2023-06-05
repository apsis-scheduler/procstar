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

    With query param `fds`, includes the bodies out outputs captured from file
    descriptors.

- `GET /procs/:id`

    Returns the current result of the process whose process ID is `:id`.  The
    format of the process result is described above.

    ```json
    {
      "proc": { ... },
    }
    ```

- `GET /procs/:id/fds/:fd`

    Returns results of a specific file descriptor, in JSON format.

- `GET /procs/:id/fds/:fd/out`

    The response body contains the raw output captured from this file
    descriptor.

    With `Accept-encoding: gz` or `Accept-encoding: br`, the response body is
    compressed correspondingly.

- `DELETE /procs/:id`

    Cleans up the specified process, if it has completed.

    `409 Conflict`: The process has not completed.

- `PUT /procs/:id/signals/:signal`

    Sends a signal to the process.

    `409 Conflict`: The process has already completed.

- `POST /procs/:id`

    Creates and starts a new process.

    `201 Created`
    `400 Bad Request`: A process with the given ID already exists.

    ```json
    {
      "proc": { ... },
    }
    ```

- `POST /procs`

    `201 Created`

    Creates and starts a new process.  A process ID is chosen automatically.
    The response `Location` header contains the URL of the new process, from
    which the process ID can be extracted.

