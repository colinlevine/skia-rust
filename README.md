# skia-rust

A safe Rust rewrite of Google's [Skia](https://skia.org/) 2D graphics library, validated against an unmodified upstream Skia baseline and its test suite.

## Repository Layout

- [`UPSTREAM.toml`](UPSTREAM.toml) — Exact pinned upstream Skia repository, branch, commit SHA, and commit date.
- [`BASELINE.toml`](BASELINE.toml) — Exact GN build flags, `dm` flags, generation date, and machine specs used for the baseline verification.
- [`PORTING.md`](PORTING.md) — Phase dependency graph, module porting order, and live verification status.
- [`rust-toolchain.toml`](rust-toolchain.toml) — Pinned Rust toolchain version (`1.100.0`).
- [`tools/verify_dm.py`](tools/verify_dm.py) — Verification harness comparing `skia-rewrite/out/Rust/dm` against `skia-baseline/out/dm/dm.json` and checking protected paths.
- [`skia-baseline/`](skia-baseline/) — Untouched pinned upstream C++ Skia checkout (read-only forever). Build artifacts (`skia-baseline/out/Original/`) and reference hashes (`skia-baseline/out/dm/dm.json`) live here and are git-ignored.
- [`skia-rewrite/`](skia-rewrite/) — Working copy of Skia containing the safe Rust crates (`skia-rewrite/rust/`), C++/Rust FFI bridges (`skia-rewrite/src/rust_bridge/`), and Rust builds (`skia-rewrite/out/Rust/`). Its `gm/`, `tests/`, `dm/`, `bench/`, and `include/` directories remain identical to `skia-baseline/`.

## Building and Running the Baseline

From `skia-baseline/`:

```bash
cd skia-baseline
GIT_SYNC_DEPS_SKIP_EMSDK=1 python3 tools/git-sync-deps --deep
python3 bin/fetch-ninja
./bin/gn gen out/Original --args='is_debug=false skia_use_fontconfig=false skia_use_system_freetype2=false skia_use_gl=false'
./third_party/ninja/ninja -C out/Original dm
./out/Original/dm --src gm tests --nogpu --config 8888 --writePath out/dm
```

## Building and Verifying the Rust Rewrite

From `skia-rewrite/`:

```bash
cd skia-rewrite
./bin/gn gen out/Rust --args='is_debug=false skia_use_fontconfig=false skia_use_system_freetype2=false skia_use_gl=false skia_rust_md5=true'
./third_party/ninja/ninja -C out/Rust dm
cd ..
python3 tools/verify_dm.py
```
