# Packaging

### Wheel - PyPI

```
$ python -m build --wheel
$ twine upload dist/procstar-...
```

### RHEL7/CentOS binary

```
$ ./tools/centos7/build.sh
$ ls -l target/centos7/release/procstar
```

