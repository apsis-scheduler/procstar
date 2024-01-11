This directory contains scripts for building Procstar in a CentOS7 container
using Podman, for compatibility with RHEL7 and similar distributions, and a
recipe for constructing a Conda package.

- `Containerfile` specifies a container image with Rust/Cargo and Miniconda.

- `build.sh` performs a Cargo release build of the local Procstar source, in the
  container.

- `build-conda.sh` performs a Cargo release build of the most recent version of
  Procstar, and wraps it in a Conda package.

The Conda recipe presumes the availability of a Rust development setup, which is
included in the container.

