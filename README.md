# skia-rust

A safe Rust rewrite of Google's [Skia](https://skia.org/) 2D graphics library, validated against an unmodified upstream Skia baseline and its test suite.

## Progress

> **Current Step:** **Phase 2 Complete (`phase2-complete`)!** — All `9/9` Phase 2 Image Codec & Encoder modules (`50` active files ported to `8` `#![forbid(unsafe_code)]` Rust crates + `10` stub/superseded files pruned; `2` disabled AVIF/JXL files deferred) are cleaned up, verified (`DM: 1000/1000`, `Phase 2: 7032/7032` image/colorImage hashes across `9,820` tasks, `2644/2644` CPU unit tests, `541/541` Rust crate tests across `26` safe Rust crates), benchmarked in **[`BENCHMARKS.md`](BENCHMARKS.md)** (`-10.7%` clean `libskia.a` build, `-5.7%` clean `dm` build, `-9.0%` `libskia.a` archive), and tagged `phase2-complete`. Ready to plan **Phase 3 (SkSL & `SkRuntimeEffect`)** upon approval.
>
> Full dependency graph & module status: **[`PORTING.md`](PORTING.md)** | Performance A/B report: **[`BENCHMARKS.md`](BENCHMARKS.md)**

```text
Phase 1 (CPU Raster 8888):  [####################]  19/19   modules (100.0%) - Complete (tagged phase1-complete)
Phase 2 (Image Codecs):     [####################]   9/9    modules (100.0%) - Complete (tagged phase2-complete)
Overall Skia Rewrite:       [#####---------------] 374/1427 files   ( 26.2%) - Phases completed: 2/6
```

| Phase | Scope | Files Ported | Share of Skia | Status |
|---|---|---|---|---|
| **Phase 1** | CPU raster backend (`config 8888`) | `314 / 314` (`19/19` modules) | `22.0%` | **Complete** (`DM: 1000/1000`, `phase1-complete`, [`BENCHMARKS.md`](BENCHMARKS.md)) |
| **Phase 2** | Image codecs (`src/codec/`, `src/encode/`) | `60 / 62` (`9/9` modules; 50 ported + 10 pruned, 2 disabled deferred) | `4.3%` | **Complete** (`7032/7032` image hashes, `phase2-complete`, [`BENCHMARKS.md`](BENCHMARKS.md)) |
| **Phase 3** | SkSL shader compiler & `SkRuntimeEffect` (`src/sksl/`, `SkRuntime*`) | `0 / 128` | `9.0%` | Pending |
| **Phase 4** | GPU backends (Ganesh, then Graphite; `src/gpu/`, `src/text/gpu/`) | `0 / 559` | `39.2%` | Pending |
| **Phase 5** | PDF, SVG, XML, XPS & `modules/` | `0 / 248` | `17.4%` | Pending |
| **Phase 6** | Core runtime, SIMD opts & platform ports (`src/ports/`, `src/opts/`, remaining `src/core/` & `src/utils/`, `src/{android,capture,lazy,sfnt}/`) | `0 / 116` | `8.1%` | Pending |

## How It's Being Ported

For the full step-by-step porting process, architecture rules, dependency graph, per-module checklist, performance A/B methodology, and post-phase cleanup workflow, see **[`PORTING.md`](PORTING.md)**.

At a high level, every module follows the same loop:

1. **Unchanged C++ API & untouched baseline:** `skia-baseline/` and `skia-rewrite/{include,gm,tests,dm,bench}` stay untouched so upstream Skia's own test suite and golden masters validate every change.
2. **Safe Rust crates + thin FFI boundary:** Each Skia subsystem is rewritten in a `#![forbid(unsafe_code)]` Rust crate under `skia-rewrite/rust/crates/`, wired behind a thin C++/Rust FFI layer (`skia-rs-ffi` + `src/rust_bridge/`) and a per-module GN toggle (`skia_rust_<module>`).
3. **Bit-exact verification on every commit:** Before landing on `main`, each module must pass all Rust unit tests, all `2644` Skia CPU unit tests, and a `1000/1000` bit-exact GM hash match against `skia-baseline/out/dm/dm.json` via [`tools/verify_dm/`](tools/verify_dm/).
4. **End-of-phase performance A/B check & cleanup:** Once a phase reaches 100%, we tag `phase<N>-pre-cleanup`, remove that phase's replaced `.cpp` files and toggles so Rust is unconditional, verify a clean build, log a baseline-vs-Rust performance comparison (runtime, compile time, binary size) in [`BENCHMARKS.md`](BENCHMARKS.md), and tag `phase<N>-complete`.

## Repository Layout

- [`UPSTREAM.toml`](UPSTREAM.toml) — Exact pinned upstream Skia repository, branch, commit SHA, and commit date.
- [`BASELINE.toml`](BASELINE.toml) — Exact GN build flags, `dm` flags, generation date, and machine specs used for the baseline verification.
- [`PORTING.md`](PORTING.md) — Step-by-step porting process, architecture ground rules, phase dependency graph, performance/cleanup workflow, and live module status.
- [`BENCHMARKS.md`](BENCHMARKS.md) — Per-phase baseline-vs-Rust performance A/B results (execution time, peak RSS, compile time, and binary size).
- [`rust-toolchain.toml`](rust-toolchain.toml) — Pinned Rust toolchain version (`1.100.0`).
- [`tools/verify_dm/`](tools/verify_dm/) — Safe Rust verification harness comparing `skia-rewrite/out/Rust/dm` against `skia-baseline/out/dm/dm.json` and checking protected paths.
- [`tools/bench_ab/`](tools/bench_ab/) — Safe Rust A/B benchmark harness comparing `skia-baseline/` vs `skia-rewrite/` (execution time, peak RSS, clean & incremental compile time, binary size).
- [`skia-baseline/`](skia-baseline/) — Untouched pinned upstream C++ Skia checkout (read-only forever). Build artifacts (`skia-baseline/out/Original/`) and reference hashes (`skia-baseline/out/dm/dm.json`) live here and are git-ignored.
- [`skia-rewrite/`](skia-rewrite/) — Working copy of Skia containing the safe Rust crates (`skia-rewrite/rust/`), C++/Rust FFI bridges (`skia-rewrite/src/rust_bridge/`), and Rust builds (`skia-rewrite/out/Rust/`). Its `gm/`, `tests/`, `dm/`, `bench/`, and `include/` directories remain identical to `skia-baseline/`.
- **Post-phase cleanup:** Original C++ files remain in `skia-rewrite/` until their phase is ported and verified (allowing incremental builds and C++/Rust bisect toggles), and each phase's replaced C++ files are cleaned up once that phase is complete.

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
./bin/gn gen out/Rust --args='is_debug=false skia_use_fontconfig=false skia_use_system_freetype2=false skia_use_gl=false'
./third_party/ninja/ninja -C out/Rust dm
cd ..
cargo run --release --manifest-path tools/verify_dm/Cargo.toml
```
