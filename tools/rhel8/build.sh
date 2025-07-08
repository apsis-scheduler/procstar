#!/bin/bash

# Builds Procstar in a Rocky Linux 8 container.

dir=$(dirname $(realpath $0))
root=$(dirname $(dirname $dir))
target=$root/target/rhel8

# Clean up previous build products.
rm -rf $target
mkdir -p $target

# Make sure the build container image is ready.
podman build $dir -t rhel8-cargo

# Build.
podman run -t --rm -v "$root:/source" -v "$target:/source/target" rhel8-cargo \
       cargo build --release

