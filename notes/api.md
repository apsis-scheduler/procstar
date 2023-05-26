
```
GET /procs

200
{
  "procs": {
    "id0": { ... },
    "id1": { ... },
  }
}
```

```
GET /procs/:id

200
{
  "proc": { ... },
}
```

```
GET /procs/{id}?include=
```

```
GET /procs/{id}/fds/{fd}
```

```
POST /procs/{id}
```

