.. _shutdown:

Shutdown
========

Signals
-------

Procstar responds as follows to signals sent to the Procstar process itself:

- On SIGQUIT, Procstar sends SIGKILL to all running processes.  It then waits up
  to 5 sec for all processes to be deleted, if invoked with `--wait`, or up to 5
  sec for all processes to terminate, if invoked with `--exit`.  Finally,
  Procstar exits.  Any remaining running or undeleted processes are orphaned.

- SIGTERM or SIGINT: Procstar sends SIGTERM to all running processes, then waits
  60 sec for all processes to complete.  Then, proceeds as for SIGQUIT.

- SIGKILL: Procstar terminates immediately; running or undeleted processes are
  orphaned.

