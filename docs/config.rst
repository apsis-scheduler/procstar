.. _config:

Configuring Procstar
====================

Restricted Executable
---------------------

By default, Procstar can start a process with any executable permitted by the
OS.  To restrict processes to a single process only, specify the `--restrict-exe
PATH` command line option.  Any process whose executable doesn't match this path
explicit fails with an error.


WebSocket Connection
--------------------

TLS
~~~

Procstar with `--connect` establishes a TLS-secured connection to a WebSocket
server.  The server must present a TLS certificate that procstar is able to
validate.  If server certificate can't be validated in the certificate chain of
one of the root certificates in the system's default certificate bundle, you can
supply an alternate certificate (either an additional root, or a self-signed
server certificate itself) by setting the environment variable
`PROCSTAR_AGENT_CERT` to point to a PEM-formatted certificate file.

The WebSocket server provided in `python.procstar.ws` will also honor
`PROCSTAR_AGENT_CERT`, if this is set, and use this as the server certificate.  The
server also requires the TLS certificate's secret key.  Set the
`PROCSTAR_AGENT_KEY` environment variable to point to this.  If this environment
variable isn't set, Procstar looks for the key at the same path as the
certificate file, but with the file suffix changed to `.key`.

Auth Token
~~~~~~~~~~

A Procstar WebSocket server can also optionally require an auth token to
authenticate incoming connections from Procstar instances.  To use such a token,
set the `PROCSTAR_AGENT_TOKEN` environment variable for both Procstar instances and
the server.  The token itself is an arbitrary string; the default is the empty
string.  A connection is accepted only if the token matches.

