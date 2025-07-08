#!/bin/bash -e

# Builds Procstar in a Rocky Linux 8 container and wraps it into a conda package.
#
# Usage: build-conda.sh

dir=$(dirname $(realpath $0))
root=$(dirname $(dirname $dir))

target=$root/target/rhel8-conda

# Clean up previous build products.
rm -rf $target
mkdir -p $target

# Make sure the build container image is ready.
podman build $dir -t rhel8-cargo

# Build.
podman run --rm \
       -v "$dir/conda-recipe:/source" -v "$target:/conda/conda-bld" \
       --workdir /root \
       rhel8-cargo \
       conda build --python 3.10 /source

echo
echo conda package: $(ls $target/linux-64/*.tar.bz2)

