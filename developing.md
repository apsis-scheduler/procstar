# Development Setup

## Python Environment with uv

This project uses [uv](https://docs.astral.sh/uv/) for Python package management. uv provides fast dependency resolution and virtual environment management.

### Quick Start

1. Install uv:
   ```bash
   curl -LsSf https://astral.sh/uv/install.sh | sh
   ```

2. Sync dependencies:
   ```bash
   uv sync
   ```

3. Run commands with uv:
   ```bash
   uv run pytest
   uv run python -m build --wheel
   ```

### Automatic Environment with direnv

For automatic environment activation, install [direnv](https://direnv.net/) and run:

```bash
direnv allow
```

This will automatically:
- Activate the uv environment when entering the directory
- Add the Python virtual environment to your PATH
- Sync dependencies if needed

After setup, you can run Python commands directly without `uv run`:
```bash
pytest
python -c "import procstar"
```


# Packaging

### Wheel - PyPI

```
$ uv run python -m build --wheel
$ twine upload dist/procstar-...
```

### RHEL8/Rocky binary

```
$ ./tools/rhel8/build.sh
$ ls -l target/rhel8/release/procstar
```

