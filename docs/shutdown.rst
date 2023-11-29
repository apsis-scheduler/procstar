.. _shutdown:

Shutdown
========

Signals
-------

Procstar responds as follows to signals sent to the Procstar process itself:

- SIGTERM or SIGINT: Procstar sends SIGTERM to all running processes.  It then
  waits up to 60 sec for all processes to complete.  Then, proceeds as for
  SIGQUIT.

- On SIGQUIT, Procstar sends SIGKILL to all running processes.  It then waits up
  to 5 sec for all processes to be deleted; during this time, it serves HTTP and
  agent connections as usual, allowing clients to obtain results from and delete
  the killed processes. Finally, Procstar itself exits.  Any remaining running
  or undeleted processes are orphaned.

- SIGKILL: Procstar terminates immediately; running or undeleted processes are
  orphaned.

