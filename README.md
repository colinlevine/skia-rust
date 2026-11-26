# skia-rs

A Rust rewrite of Google's [Skia](https://skia.org/) 2D graphics library, validated against an unmodified upstream Skia baseline and its test suite.

## Repository Layout

- [`UPSTREAM.toml`](UPSTREAM.toml) — Exact pinned upstream Skia repository, branch, commit SHA, and commit date.
- [`baseline/`](baseline/) — Unmodified pinned upstream C++ Skia checkout, including its unchanged `tests/` and `gm/` suites as the source of truth. Build artifacts, synced third-party dependencies, and generated reference outputs live inside `baseline/` and are git-ignored via [`baseline/.gitignore`](baseline/.gitignore).
- [`BASELINE.toml`](BASELINE.toml) — Exact GN build flags, `dm` flags, generation date, and machine specs used for the baseline verification.

## Building and Running the Baseline

All upstream Skia build artifacts and reference outputs stay inside `baseline/out/` (untracked by Git).

### 1. Sync pinned dependencies and build tools

```bash
cd baseline
GIT_SYNC_DEPS_SKIP_EMSDK=1 python3 tools/git-sync-deps --deep
python3 bin/fetch-ninja
```

### 2. Configure with GN

```bash
./bin/gn gen out/Original --args='is_debug=false skia_use_fontconfig=false skia_use_system_freetype2=false skia_use_gl=false'
```

### 3. Build the `dm` test runner (and `nanobench` for benchmarking)

```bash
./third_party/ninja/ninja -C out/Original dm
```

### 4. Run tests and generate reference outputs

Run all CPU unit tests (`2644` tests) and Golden Master rendering tests (`1082` GMs), writing reference hashes (`dm.json`) and PNGs to `baseline/out/dm/`:

```bash
./out/Original/dm --src gm tests --nogpu --config 8888 --writePath out/dm
```

To run only the unit tests without writing PNGs:

```bash
./out/Original/dm --src tests --nogpu --config 8888
```
