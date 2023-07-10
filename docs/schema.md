## Specs

A _spec_ is a JSON-serializable structure that specifies in detail how to run a
process or processes.

The `procs` key is a dictionary from proc ID to proc spec.  To start a new
process, the proc ID may not already exist (though a proc ID may be deleted and
reused).

```json
{
  "procs": {
    PROCID: PROC, ...
  }
}
```

### Proc specs

A proc spec specifies at least the argv for the process, and optionally
additional details about the process.

```json
{
  "argv": ["argv0", "argv1", ...],
  "env": ENV,
  "fds": FDS,
}
```

The optional `env` key specifies the process enviornment.  If omitted, the
process inherits the environment from Procstar.

The optional `fds` key specifies open file descriptors at startup.  If omitted,
the process inherits stdin (fd 0), stdout (fd 1), and stderr (fd 2) from
Procstar.


### Env specs

```json
{
  "inherit": true | false | ["name", ...],
  "vars": {"name": "value", ...}
}
```

The `inherit` key determines which environment variables are inherited from the
Procstar process.  It may be,
- `true` to inherit everything (the default)
- `false` to inherit nothing
- a list of environment variable names to inherit; a name that is not present in
  Procstar's environment is ignored

The `vars` key specifies environment variable names and values to set in the
process.  These override any inherite environment variables.

