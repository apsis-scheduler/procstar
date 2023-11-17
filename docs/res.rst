Process results
---------------

A process result object has these attributes:

- `state`: One of,

    - `"error"`: The specified process did not start, due to a failure in
      setting up file descriptors or in fork or exec.

    - `"running"`: The process started and has not yet terminated.

    - `"terminated"`: The process started and then terminated, either by
      completing normally or by a signal.

- `error`: An array of strings indicating why the specified process did not start.

- `status`: The process termination status.  Its fields are,

    - `exit_code`: The exit code, if the process terminated with `exit()`.

    - `signum`: The signal number that terminated the process, if any.

    - `signal`: The name of the signal that terminated the process, if any.

    - `core_dump`: Whether the process produced a core dump on termination.

- `times`: A structure of timing information.

    - `start`: Approximate system time when the process started.

    - `stop`: Approximate system time when the process terminated.

    - `elapsed`: Approximate elapsed time between process start and termination,
      according to the system's monotonic clock.

FIXME: Finish.
