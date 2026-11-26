# skia-rust

A safe Rust rewrite of Google's [Skia](https://skia.org/) 2D graphics library, validated against an unmodified upstream Skia baseline and its test suite.

## Progress

> **Current Step:** **Phase 1 — Layer 2 (Core Primitives & Data Structures)** — Completed `skia-rs-core-types` (18 files: `SkData`, `SkStream`, `SkBuffer`, `SkRead/WriteBuffer`, `SkWriter32`, `SkString`, `SkFlattenable`, `SkPaint`, `SkDescriptor`, `SkMask`, `SkBlurMask`, `SkGaussFilter`, `SkMaskGamma`, `SkMaskCache`; `DM: 1000/1000 match`, `2644/2644` CPU unit tests); finishing `skia-rs-color`, `skia-rs-path`, and `skia-rs-region` in parallel.
>
> Full dependency graph & module status: **[`PORTING.md`](PORTING.md)**

```text
Phase 1 (CPU Raster 8888):  [#####---------------]   5/19   modules (26.3%) - Layer 2 in progress
Overall Skia Rewrite:       [#-------------------]  48/1442 files   ( 3.3%) - Phases completed: 0/5
```

| Phase | Scope | Files Ported | Share of Skia | Status |
|---|---|---|---|---|
| **Phase 1** | CPU raster backend (`config 8888`) | `48 / 448` (`5/19` modules) | `31.1%` | In progress (`skia-rs-core-types` done; `color`/`path`/`region` in flight) |
| **Phase 2** | Image codecs (`src/codec/`, `src/encode/`) | `0 / 62` | `4.3%` | Pending |
| **Phase 3** | SkSL shader compiler (`src/sksl/`) | `0 / 121` | `8.4%` | Pending |
| **Phase 4** | GPU backends (Ganesh, then Graphite) | `0 / 544` | `37.7%` | Pending |
| **Phase 5** | PDF, SVG, XML, and `modules/` | `0 / 267` | `18.5%` | Pending |

## Repository Layout

- [`UPSTREAM.toml`](UPSTREAM.toml) — Exact pinned upstream Skia repository, branch, commit SHA, and commit date.
- [`BASELINE.toml`](BASELINE.toml) — Exact GN build flags, `dm` flags, generation date, and machine specs used for the baseline verification.
- [`PORTING.md`](PORTING.md) — Phase dependency graph, module porting order, and live verification status.
- [`rust-toolchain.toml`](rust-toolchain.toml) — Pinned Rust toolchain version (`1.100.0`).
- [`tools/verify_dm/`](tools/verify_dm/) — Safe Rust verification harness comparing `skia-rewrite/out/Rust/dm` against `skia-baseline/out/dm/dm.json` and checking protected paths.
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
cargo run --release --manifest-path tools/verify_dm/Cargo.toml
```
