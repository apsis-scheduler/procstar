# Packaging

### Wheel - PyPI

```
$ python -m build --wheel
$ twine upload dist/procstar-...
```

### RHEL8/Rocky binary

```
$ ./tools/rhel8/build.sh
$ ls -l target/rhel8/release/procstar
```

