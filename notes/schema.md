```json
{
  "proc": {
    "argv": [
      "/usr/bin/echo",
      "Hello, world!"
    ]
  }
}
```

results in,

```json
{
  "proc": {
    "pid": 12345,
    "status": 0,
    "exit_code': 0,
    "fds": {
      "stdout": "Hello, world!\n"
    }
  }
}
```

```json
{
  "procs": {
    "0": {
      "argv": [
        "/usr/bin/echo",
        "Hello, world!"
      ]
    },
    "1": {
      "argv": [
        "/usr/bin/echo",
        "Good bye."
      ]
    }
}
```

results in,

```json
{
  "procs": {
    "0": {
      "pid": 12345,
      "status": 0,
      "exit_code': 0,
      "fds": {
        "stdout": "Hello, world!\n"
      }
    },
    "1": {
      "pid": 23456,
      "status": 0,
      "exit_code': 0,
      "fds": {
        "stdout": "Good bye.\n"
      }
    }
  }
}
```

