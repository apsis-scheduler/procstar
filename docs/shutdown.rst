.. _shutdown:

Shutdown
========

Signals
-------

Procstar responds as follows to signals sent to the Procstar process itself:

- SIGTERM or SIGINT: Procstar sends SIGTERM to all running processes, then waits
  60 sec for all processes to complete.  If any processes are still running,
  Procstar sends SIGKILL to them, waits 5 sec, and then exits.

- SIGQUIT: Procstar sends SIGKILL to all running processes, waits 5 sec, and
  then exits.

- SIGKILL: Procstar terminates immediately; running processes are orphaned.

