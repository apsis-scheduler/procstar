#!/bin/bash

# Builds Procstar in a CentOS7 container.

dir=$(dirname $(realpath $0))
root=$(dirname $(dirname $dir))
target=$root/target/centos7

# Clean up previous build products.
rm -rf $target
mkdir -p $target

# Make sure the build container image is ready.
podman build $dir -t centos7-cargo

# Build.
podman run -t --rm -v "$root:/source" -v "$target:/source/target" centos7-cargo \
       cargo build --release

