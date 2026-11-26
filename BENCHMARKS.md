# Skia Rust Rewrite — Per-Phase Performance A/B Benchmarks

This file records reproducible A/B performance comparisons between the untouched upstream C++ baseline ([`skia-baseline/`](skia-baseline/)) and the safe Rust rewrite ([`skia-rewrite/`](skia-rewrite/)) after each completed phase (see [`PORTING.md`](PORTING.md) Section 5).

---

## Machine & Build Configuration

- **Pinned Upstream Skia:** `67badb88843a848541ffb57fb0e7a3fc88c48051` (`main`, `2026-11-25`)
- **Rust Toolchain:** `1.100.0` (`rust-toolchain.toml`, `profile.release`: `opt-level = 3`, `lto = "thin"`, `codegen-units = 1`, `panic = "abort"`)
- **GN Args:** `is_debug=false skia_use_fontconfig=false skia_use_system_freetype2=false skia_use_gl=false`
- **Host:** `x86_64`, Linux, AVX2, 20 CPU cores, 15 GiB RAM

---

## Phase 1 — CPU Raster Backend (`config 8888`)

- **Date:** `2026-11-26`
- **Scope Ported & Cleaned Up:** `19/19` modules (`314` C++ files deleted from `skia-rewrite/src/` and replaced unconditionally by `18` `#![forbid(unsafe_code)]` Rust crates + `skia-rs-ffi` + `80` thin C++ bridge shims; pre-cleanup toggleable state tagged at `phase1-pre-cleanup`).
- **Correctness:** `DM: 1000/1000` bit-exact GM MD5 match, `2644/2644` Skia CPU unit tests pass, `427/427` Rust crate unit tests pass.

### 1. Execution Time & Peak Memory (Median of 3 Runs)

| Workload | Baseline C++ (`skia-baseline/out/Original/dm`) | Rust Rewrite (`skia-rewrite/out/Rust/dm`) | Delta |
|---|---:|---:|---:|
| **`--src gm --nogpu --config 8888`** (1,000 GMs, wall time) | `7.40s` *(runs: 7.40, 7.38, 8.40)* | **`6.89s`** *(runs: 6.89, 6.46, 8.42)* | **`-6.9%` (faster)** |
| **`--src gm --nogpu --config 8888`** (peak RSS) | `1,131 MB` | **`1,099 MB`** | **`-32 MB` (`-2.8%`)** |
| **`--src tests --nogpu --config 8888`** (2,644 CPU unit tests, wall time) | `12.69s` *(runs: 13.40, 12.69, 12.43)* | `13.27s` *(runs: 13.95, 13.27, 13.26)* | `+4.6%` (`+0.58s`) |
| **`--src tests --nogpu --config 8888`** (peak RSS) | `696 MB` | **`670 MB`** | **`-26 MB` (`-3.7%`)** |

### 2. Compile Time

| Build Measurement | Baseline C++ | Rust Rewrite (Phase 1 Unconditional) | Delta |
|---|---:|---:|---:|
| **Clean `libskia.a` build (`gn gen` + `ninja libskia.a`)** | `308.84s` *(gn `0.19s` + C++ `308.65s`)* | **`290.52s`** *(gn `0.17s` + Rust `11.56s` + C++ `278.78s`)* | **`-5.9%` (`-18.32s` faster)** |
| **Clean Rust workspace (`cargo build --release` across all 19 crates)** | — | `10.64s` *(11.59s via ninja)* | — |
| **Clean full `dm` build (`gn gen` + `ninja dm`, 2,842 targets incl. `gm/` & `tests/`)** | — | `774.02s` *(gn `0.19s` + Rust `11.59s` + C++/link `762.24s`)* | — |
| **Incremental `dm` rebuild — single `.cpp` touch (`SkMath_rust.cpp`)** | — | `29.16s` *(includes full `dm` link)* | — |
| **Incremental `dm` rebuild — root `.rs` touch (`skia-rs-math/src/lib.rs`)** | — | `39.05s` *(rebuilds dependent crates + thin-LTO staticlib + `dm` link)* | — |
| **Incremental `dm` rebuild — leaf `.rs` touch (`skia-rs-canvas/src/lib.rs`)** | — | `45.65s` *(rebuilds crate + thin-LTO staticlib + `dm` link)* | — |

*Note: Clean baseline `libskia.a` was timed in an isolated `/tmp` build directory with read-only symlinks to `skia-baseline/` so `skia-baseline/` remained completely untouched.*

### 3. Binary & Archive Size

| Artifact | Baseline C++ | Rust Rewrite | Delta |
|---|---:|---:|---:|
| **`dm` (unstripped)** | `862.45 MB` (`862,448,536 B`) | **`831.51 MB`** (`831,506,944 B`) | **`-3.6%` (`-30.94 MB`)** |
| **`dm` (stripped)** | `41.45 MB` (`41,451,952 B`) | `42.75 MB` (`42,753,120 B`) | `+3.1%` (`+1.30 MB`) |
| **`libskia.a` (+ `libskia_rs_ffi.a`)** | `1,171.54 MB` (`1,171,537,760 B`) | **`1,079.83 MB`** (`1,066.83 MB` + `13.00 MB`) | **`-7.8%` (`-91.71 MB`)** |

### 4. Summary Assessment

No performance regressions were observed for Phase 1:
- **Runtime:** Full 1,000-GM CPU rasterization is **6.9% faster** with **2.8% lower peak memory**; all 2,644 CPU unit tests are within `+4.6%` wall time (`+0.58s`) with **3.7% lower peak memory**.
- **Compile time:** Clean `libskia.a` compilation is **5.9% faster** than upstream C++ — all 18 safe Rust crates plus `skia-rs-ffi` compile from scratch in **10.64s**.
- **Binary size:** Stripped `dm` is within `+3.1%` (`+1.3 MB`), and unstripped `dm` and `libskia.a` archives are smaller (`-3.6%` and `-7.8%`).
