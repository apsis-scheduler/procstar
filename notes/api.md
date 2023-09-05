
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

---

# Websockets

## Incoming

Please start a new process and send `proc-update`.
```json
{
  "type": "proc-start",
  "id": ...,
  "spec": ...
}
```

Please send `procid-list`.
```json
{
  "type": "procid-list-request"
}
```

Please send `proc-result`.
```json
{
  "type": "proc-result-request",
  "id": ...
}
```

Please clean up a proc and send `proc-delete`.
```json
{
  "type": "proc-delete-request",
  "id": ...
}
```



## Outgoing

```json
{
  "type": "procid-list",
  "procids": [...]
}
```

```json
{
  "type": "proc-result",
  "id": ...,
  "res": ...
}
```

```json
{
  "type": "proc-delete",
  "id": ...
}
```

