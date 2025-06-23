Procstar is a tool for running Linux processes from declarative specifications.
Procstar also babysits these processes, possibly managing inputs and outputs,
and collecting process status.  Procstar may be run as a long-running agent
service, accepting comamnds via HTTP or WebSocket APIs to start and manage
processes.

Additionally, procstar integrates with cgroups on systemd distros with the unified
hierarchy.

- Limit resources (memory, swap, tasks).
- Read cgroup accounting information for full process tree after exit.
- Terminate all orphaned processes in tree after the originally forked process exits.

You can use Procstar agents as execution agents for the [Apsis
scheduler](https://apsis-scheduler/apsis).

