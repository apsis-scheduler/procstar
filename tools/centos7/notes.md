This directory contains scripts for building Procstar in a CentOS7 container
using Podman, for compatibility with RHEL7 and similar distributions.

`Containerfile` specifies a container image with Rust/Cargo and Miniconda.

`build.sh` performs a Cargo release build of the local Procstar source, in the
container.

`build-conda.sh VERSION` performs a Cargo release build of Procstar from a git
tag, and wraps it in a conda package.

