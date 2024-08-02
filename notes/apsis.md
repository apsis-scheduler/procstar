```
# Procstar
TZ=UTC PROCSTAR_AGENT_TOKEN=foobar PROCSTAR_AGENT_CERT=$(pwd)/python/procstar/testing/localhost.crt PROCSTAR_AGENT_HOST=localhost cargo run -- --agent --log-level trace

# Apsis
PROCSTAR_AGENT_TOKEN=foobar PROCSTAR_AGENT_CERT=../procstar/python/procstar/ws/localhost.crt apsisctl --log DEBUG serve --config config.yaml
```
