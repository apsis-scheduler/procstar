#!/bin/bash -e

# Builds Procstar in a CentOS7 container and wraps it into a conda package.
#
# Usage: build-conda.sh

dir=$(dirname $(realpath $0))
root=$(dirname $(dirname $dir))

target=$root/target/centos7-conda

# Clean up previous build products.
rm -rf $target
mkdir -p $target

# Make sure the build container image is ready.
podman build $dir -t centos7-cargo

# Build.
podman run --rm \
       -v "$dir/conda-recipe:/source" -v "$target:/conda/conda-bld" \
       --workdir /root \
       centos7-cargo \
       conda build /source

echo
echo conda package: $(ls $target/linux-64/*.tar.bz2)

