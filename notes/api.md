
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
  "type": "ProcStart",
  "id": ...,
  "spec": ...
}
```

Please send `procid-list`.
```json
{
  "type": "ProcidListReqyest"
}
```

Please send `proc-result`.
```json
{
  "type": "ProcResultRequest",
  "id": ...
}
```

Please clean up a proc and send `proc-delete`.
```json
{
  "type": "ProcDeleteRequest",
  "id": ...
}
```



## Outgoing

```json
{
  "type": "ProcidList",
  "procids": [...]
}
```

```json
{
  "type": "ProcResult",
  "id": ...,
  "res": ...
}
```

```json
{
  "type": "ProcDelete",
  "id": ...
}
```

