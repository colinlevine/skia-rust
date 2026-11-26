#!/usr/bin/env python3
"""GN action script to build the Rust skia-rs-ffi staticlib via Cargo."""

import argparse
import os
import shutil
import subprocess
import sys


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", required=True)
    parser.add_argument("--target-dir", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--depfile", required=True)
    args = parser.parse_args()

    env = os.environ.copy()
    cargo_bin = os.path.expanduser("~/.cargo/bin")
    if os.path.isdir(cargo_bin):
        env["PATH"] = cargo_bin + os.pathsep + env.get("PATH", "")

    manifest = os.path.abspath(args.manifest)
    target_dir = os.path.abspath(args.target_dir)
    output = os.path.abspath(args.output)
    depfile = os.path.abspath(args.depfile)

    cmd = [
        "cargo",
        "build",
        "--release",
        "--manifest-path",
        manifest,
        "--target-dir",
        target_dir,
        "--package",
        "skia-rs-ffi",
    ]
    res = subprocess.run(cmd, env=env)
    if res.returncode != 0:
        return res.returncode

    built_lib = os.path.join(target_dir, "release", "libskia_rs_ffi.a")
    os.makedirs(os.path.dirname(output), exist_ok=True)
    shutil.copy2(built_lib, output)

    # Localize weak math/compiler_builtins symbols in the Rust staticlib so they
    # cannot shadow glibc libm symbols (e.g. cbrtf) when linked into C++ binaries.
    nm_res = subprocess.run(
        ["nm", "-g", "--defined-only", "--format=posix", output],
        capture_output=True,
        text=True,
        check=True,
    )
    weak_syms: set[str] = set()
    for line in nm_res.stdout.splitlines():
        parts = line.split()
        if len(parts) >= 2 and parts[1] == "W" and not parts[0].startswith("skia_rs_"):
            weak_syms.add(parts[0])
    if weak_syms:
        sym_file = output + ".localize_syms"
        with open(sym_file, "w", encoding="utf-8") as f:
            for sym in sorted(weak_syms):
                f.write(sym + "\n")
        subprocess.run(
            ["objcopy", f"--localize-symbols={sym_file}", output],
            check=True,
        )
        os.remove(sym_file)

    # Convert Cargo's depfile to reference Ninja's output path.
    cargo_dep = os.path.join(target_dir, "release", "libskia_rs_ffi.d")
    deps: set[str] = {manifest}
    if os.path.exists(cargo_dep):
        with open(cargo_dep, "r", encoding="utf-8") as f:
            content = f.read()
        for line in content.splitlines():
            if ":" in line:
                _, rhs = line.split(":", 1)
                for tok in rhs.strip().split():
                    if os.path.exists(tok):
                        deps.add(tok)

    # Also include all Cargo.toml files in the workspace.
    ws_root = os.path.dirname(manifest)
    for root, _, files in os.walk(ws_root):
        for fn in files:
            if fn in ("Cargo.toml", "Cargo.lock") or fn.endswith(".rs"):
                deps.add(os.path.join(root, fn))

    os.makedirs(os.path.dirname(depfile), exist_ok=True)
    with open(depfile, "w", encoding="utf-8") as f:
        dep_list = " ".join(sorted(deps))
        f.write(f"{args.output}: {dep_list}\n")

    return 0


if __name__ == "__main__":
    sys.exit(main())
