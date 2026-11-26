# Pinned Upstream Skia Baseline

This repository tracks a Rust rewrite of Skia against an exact, unmodified upstream C++ Skia baseline stored in [`baseline/`](baseline/).

## Pinned Upstream Version

- **Repository:** `https://skia.googlesource.com/skia.git`
- **Commit:** `67badb88843a848541ffb57fb0e7a3fc88c48051`
- **Date:** `2026-11-25 15:00:03 -0800`
- **Subject:** `Roll vulkan-deps from c385bc289d95 to c9e05a312715 (8 revisions)`
- **External dependencies:** Pinned by SHA1 in [`baseline/DEPS`](baseline/DEPS)

## Reproducing the Baseline Build & Tests

From `baseline/`:

1. Sync pinned third-party dependencies and fetch `gn` and `ninja`:
   ```bash
   cd baseline
   GIT_SYNC_DEPS_SKIP_EMSDK=1 python3 tools/git-sync-deps --deep
   python3 bin/fetch-ninja
   ```
2. Generate Ninja build files (standalone Linux build with bundled FreeType2, no fontconfig/GL headers required):
   ```bash
   ./bin/gn gen out/Release --args='is_debug=false skia_use_fontconfig=false skia_use_system_freetype2=false skia_use_gl=false'
   ```
3. Build the `dm` test runner:
   ```bash
   ./third_party/ninja/ninja -C out/Release dm
   ```
4. Run the baseline CPU test suites:
   ```bash
   ./out/Release/dm --src tests --nogpu --config 8888
   ./out/Release/dm --src gm --nogpu --config 8888
   ```

## Baseline Verification Results

Verified on Ubuntu 22.04.5 LTS (`x86_64`, GCC `11.4.0`, GN `2175`, Ninja `1.12.1`):

- **Build (`dm`):** `2996/2996` targets built and linked cleanly (exit code `0`)
- **CPU Unit Tests (`--src tests --nogpu --config 8888`):** `2644/2644` passed (exit code `0`)
- **CPU Golden Master Tests (`--src gm --nogpu --config 8888`):** `1082/1082` passed (exit code `0`)

## Ground Rules

- `baseline/tests/` and `baseline/gm/` remain unchanged throughout the rewrite as the final source of truth.
- Synced external repos (`baseline/third_party/externals/`), fetched tools (`bin/gn`, `third_party/ninja/`), and build artifacts (`baseline/out/`) are git-ignored via `baseline/.gitignore`.
