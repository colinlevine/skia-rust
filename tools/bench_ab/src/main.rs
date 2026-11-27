#![forbid(unsafe_code)]

//! Safe Rust A/B performance benchmarking harness for `skia-rust`.
//!
//! Compares `skia-baseline/` (untouched upstream C++ Skia, never written to)
//! against `skia-rewrite/` (safe Rust rewrite) across:
//! 1. Execution time & peak RSS (`--src gm --nogpu --config 8888` and `--src tests`, median of N runs).
//! 2. Clean build time (`libskia.a` and/or full `dm`, breaking out GN / Rust / C++ + link)
//!    using isolated `/tmp` symlink trees so `skia-baseline/` stays strictly read-only.
//! 3. Single-file incremental rebuild time (`.cpp` touch vs root/leaf `.rs` touch).
//! 4. Binary & archive sizes (unstripped & stripped `dm`, `libskia.a`, `libskia_rs_ffi.a`).
//!
//! Usage:
//!   cargo run --release --manifest-path tools/bench_ab/Cargo.toml -- [OPTIONS]

use std::env;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Instant, SystemTime};

const DEFAULT_GN_ARGS: &str =
    "is_debug=false skia_use_fontconfig=false skia_use_system_freetype2=false skia_use_gl=false";

struct Config {
    repo_root: PathBuf,
    gn_args: String,
    runs: usize,
    run_sizes: bool,
    run_exec: bool,
    run_clean_libskia: bool,
    run_clean_dm: bool,
    run_incr: bool,
    tmp_root: PathBuf,
}

fn parse_args() -> Result<Config, String> {
    let default_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let mut cfg = Config {
        repo_root: default_root,
        gn_args: DEFAULT_GN_ARGS.to_string(),
        runs: 3,
        run_sizes: false,
        run_exec: false,
        run_clean_libskia: false,
        run_clean_dm: false,
        run_incr: false,
        tmp_root: PathBuf::from("/tmp/skia-rs-bench-ab"),
    };

    let mut any_section = false;
    let mut it = env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--repo-root" => {
                cfg.repo_root = PathBuf::from(it.next().ok_or("--repo-root requires a value")?);
            }
            "--gn-args" => {
                cfg.gn_args = it.next().ok_or("--gn-args requires a value")?;
            }
            "--runs" => {
                let s = it.next().ok_or("--runs requires a value")?;
                cfg.runs = s.parse::<usize>().map_err(|_| format!("Invalid --runs: {s}"))?;
                if cfg.runs == 0 {
                    return Err("--runs must be >= 1".to_string());
                }
            }
            "--tmp-root" => {
                cfg.tmp_root = PathBuf::from(it.next().ok_or("--tmp-root requires a value")?);
            }
            "--sizes" => {
                cfg.run_sizes = true;
                any_section = true;
            }
            "--exec" => {
                cfg.run_exec = true;
                any_section = true;
            }
            "--clean-libskia" => {
                cfg.run_clean_libskia = true;
                any_section = true;
            }
            "--clean-dm" => {
                cfg.run_clean_dm = true;
                any_section = true;
            }
            "--incr" => {
                cfg.run_incr = true;
                any_section = true;
            }
            "--all" => {
                cfg.run_sizes = true;
                cfg.run_exec = true;
                cfg.run_clean_libskia = true;
                cfg.run_clean_dm = true;
                cfg.run_incr = true;
                any_section = true;
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            other => return Err(format!("Unknown argument: {other}")),
        }
    }

    if !any_section {
        // Default fast+complete suite (skips full 13-min clean dm unless --clean-dm or --all is passed).
        cfg.run_sizes = true;
        cfg.run_exec = true;
        cfg.run_clean_libskia = true;
        cfg.run_incr = true;
    }

    Ok(cfg)
}

fn print_help() {
    println!(
        "Usage: bench-ab [OPTIONS]\n\
         \n\
         Sections (default if none specified: --sizes --exec --clean-libskia --incr):\n\
           --sizes            Compare unstripped/stripped dm, libskia.a, and libskia_rs_ffi.a sizes\n\
           --exec             Run 3-run median execution time & peak RSS on gm and tests\n\
           --clean-libskia    Clean gn + ninja libskia.a build time (baseline vs rewrite)\n\
           --clean-dm         Clean gn + ninja dm build time (baseline vs rewrite, ~12-13 min each)\n\
           --incr             Incremental single-file dm rebuild times (.cpp vs root/leaf .rs)\n\
           --all              Run all sections including --clean-dm\n\
         \n\
         Options:\n\
           --runs N           Number of runs per execution workload (default: 3)\n\
           --gn-args STRING   GN args string (default: baseline CPU 8888 args)\n\
           --repo-root PATH   Repository root path\n\
           --tmp-root PATH    Temp directory for isolated builds (default: /tmp/skia-rs-bench-ab)"
    );
}

fn format_delta(base: f64, rust: f64) -> String {
    if base <= 0.0 {
        return "—".to_string();
    }
    let pct = ((rust - base) / base) * 100.0;
    format!("{pct:+.1}%")
}

fn format_mb(bytes: u64) -> String {
    format!("{:.2} MB", (bytes as f64) / 1_000_000.0)
}

fn median(mut vals: Vec<f64>) -> f64 {
    vals.sort_by(|a, b| a.total_cmp(b));
    let n = vals.len();
    if n == 0 {
        0.0
    } else if n % 2 == 1 {
        vals[n / 2]
    } else {
        (vals[n / 2 - 1] + vals[n / 2]) * 0.5
    }
}

fn parse_peak_mb(output: &str) -> u64 {
    let mut max_mb = 0u64;
    for token in output.split_whitespace() {
        if let Some(num) = token.strip_suffix("MB") {
            if let Ok(v) = num.parse::<u64>() {
                // Peak values appear as `<N>MB peak`; checking adjacent token is even cleaner:
                let _ = v;
            }
        }
    }
    let tokens: Vec<&str> = output.split_whitespace().collect();
    for w in tokens.windows(2) {
        if w[1].starts_with("peak") {
            if let Some(num) = w[0].strip_suffix("MB") {
                if let Ok(v) = num.parse::<u64>() {
                    if v > max_mb {
                        max_mb = v;
                    }
                }
            }
        }
    }
    max_mb
}

fn run_capture(cmd: &mut Command) -> Result<(f64, String), String> {
    let t0 = Instant::now();
    let out = cmd
        .output()
        .map_err(|e| format!("Failed to spawn {:?}: {e}", cmd.get_program()))?;
    let dt = t0.elapsed().as_secs_f64();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !out.status.success() {
        return Err(format!(
            "Command {:?} failed with {}:\n{}",
            cmd.get_program(),
            out.status,
            combined.lines().rev().take(20).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n")
        ));
    }
    Ok((dt, combined))
}

fn run_quiet(cmd: &mut Command) -> Result<f64, String> {
    let t0 = Instant::now();
    let status = cmd
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("Failed to spawn {:?}: {e}", cmd.get_program()))?;
    let dt = t0.elapsed().as_secs_f64();
    if !status.success() {
        return Err(format!("Command {:?} failed with {status}", cmd.get_program()));
    }
    Ok(dt)
}

fn touch_file(path: &Path) -> Result<(), String> {
    let f = fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|e| format!("open {}: {e}", path.display()))?;
    f.set_modified(SystemTime::now())
        .map_err(|e| format!("set_modified {}: {e}", path.display()))
}

/// Creates a symlinked mirror of `src_dir` inside `dst_dir` (excluding `out/`).
/// If `copy_rel` is provided (e.g. `src/core/SkMathPriv.cpp`), only that file is copied
/// into a real directory so touching it never alters mtimes in `src_dir`.
fn setup_isolated_tree(
    src_dir: &Path,
    dst_dir: &Path,
    copy_rel: Option<&Path>,
) -> Result<(), String> {
    if dst_dir.exists() {
        fs::remove_dir_all(dst_dir).map_err(|e| format!("remove {}: {e}", dst_dir.display()))?;
    }
    fs::create_dir_all(dst_dir).map_err(|e| format!("create_dir {}: {e}", dst_dir.display()))?;
    mirror_dir(src_dir, dst_dir, copy_rel, true)
}

fn mirror_dir(
    src_dir: &Path,
    dst_dir: &Path,
    copy_rel: Option<&Path>,
    skip_out: bool,
) -> Result<(), String> {
    let mut components = copy_rel.map(|p| p.components());
    let next_comp = components
        .as_mut()
        .and_then(|it| it.next())
        .map(|c| c.as_os_str().to_os_string());
    let rest_rel: Option<PathBuf> = components.and_then(|it| {
        let p: PathBuf = it.collect();
        if p.as_os_str().is_empty() {
            None
        } else {
            Some(p)
        }
    });

    for entry in fs::read_dir(src_dir).map_err(|e| format!("read_dir {}: {e}", src_dir.display()))? {
        let entry = entry.map_err(|e| format!("dir entry: {e}"))?;
        let name = entry.file_name();
        if skip_out && name == "out" {
            continue;
        }
        let s = entry.path();
        let d = dst_dir.join(&name);

        if next_comp.as_deref() == Some(name.as_os_str()) {
            if let Some(ref rest) = rest_rel {
                fs::create_dir_all(&d).map_err(|e| format!("mkdir {}: {e}", d.display()))?;
                mirror_dir(&s, &d, Some(rest.as_path()), false)?;
            } else {
                fs::copy(&s, &d).map_err(|e| format!("copy {} -> {}: {e}", s.display(), d.display()))?;
            }
        } else {
            symlink(&s, &d).map_err(|e| format!("symlink {} -> {}: {e}", s.display(), d.display()))?;
        }
    }
    Ok(())
}

fn bench_sizes(cfg: &Config) -> Result<(), String> {
    let base_out = cfg.repo_root.join("skia-baseline/out/Original");
    let rust_out = cfg.repo_root.join("skia-rewrite/out/Rust");
    let sizes_tmp = cfg.tmp_root.join("sizes");
    let _ = fs::remove_dir_all(&sizes_tmp);
    fs::create_dir_all(&sizes_tmp).map_err(|e| format!("mkdir {}: {e}", sizes_tmp.display()))?;

    let base_dm_src = base_out.join("dm");
    let rust_dm_src = rust_out.join("dm");
    let base_dm_copy = sizes_tmp.join("dm-base");
    let rust_dm_copy = sizes_tmp.join("dm-rust");
    fs::copy(&base_dm_src, &base_dm_copy)
        .map_err(|e| format!("copy {}: {e}", base_dm_src.display()))?;
    fs::copy(&rust_dm_src, &rust_dm_copy)
        .map_err(|e| format!("copy {}: {e}", rust_dm_src.display()))?;

    let base_unstripped = fs::metadata(&base_dm_copy).map_err(|e| e.to_string())?.len();
    let rust_unstripped = fs::metadata(&rust_dm_copy).map_err(|e| e.to_string())?.len();
    run_quiet(Command::new("strip").arg(&base_dm_copy).arg(&rust_dm_copy))?;
    let base_stripped = fs::metadata(&base_dm_copy).map_err(|e| e.to_string())?.len();
    let rust_stripped = fs::metadata(&rust_dm_copy).map_err(|e| e.to_string())?.len();

    let base_libskia = fs::metadata(base_out.join("libskia.a"))
        .map_err(|e| e.to_string())?
        .len();
    let rust_libskia = fs::metadata(rust_out.join("libskia.a"))
        .map_err(|e| e.to_string())?
        .len();
    let ffi_a = fs::metadata(rust_out.join("libskia_rs_ffi.a"))
        .map_err(|e| e.to_string())?
        .len();
    let rust_total_lib = rust_libskia + ffi_a;

    let _ = fs::remove_dir_all(&sizes_tmp);

    println!("\n### Binary & Archive Size\n");
    println!("| Artifact | Baseline C++ | Rust Rewrite | Delta |");
    println!("|---|---:|---:|---:|");
    println!(
        "| `dm` (unstripped) | {} | {} | {} |",
        format_mb(base_unstripped),
        format_mb(rust_unstripped),
        format_delta(base_unstripped as f64, rust_unstripped as f64)
    );
    println!(
        "| `dm` (stripped) | {} | {} | {} |",
        format_mb(base_stripped),
        format_mb(rust_stripped),
        format_delta(base_stripped as f64, rust_stripped as f64)
    );
    println!(
        "| `libskia.a` (+ `libskia_rs_ffi.a`) | {} | {} ({} + {}) | {} |",
        format_mb(base_libskia),
        format_mb(rust_total_lib),
        format_mb(rust_libskia),
        format_mb(ffi_a),
        format_delta(base_libskia as f64, rust_total_lib as f64)
    );
    Ok(())
}

fn bench_exec(cfg: &Config) -> Result<(), String> {
    let base_dir = cfg.repo_root.join("skia-baseline");
    let rust_dir = cfg.repo_root.join("skia-rewrite");
    let base_dm = base_dir.join("out/Original/dm");
    let rust_dm = rust_dir.join("out/Rust/dm");

    println!("\n### Execution Time & Peak Memory (Median of {} Runs)\n", cfg.runs);
    println!("| Workload | Baseline C++ | Rust Rewrite | Delta |");
    println!("|---|---:|---:|---:|");

    for (label, extra_args, needs_write) in [
        ("--src gm --nogpu --config 8888", vec!["--src", "gm", "--nogpu", "--config", "8888"], true),
        ("--src tests --nogpu --config 8888", vec!["--src", "tests", "--nogpu", "--config", "8888"], false),
    ] {
        let mut b_times = Vec::with_capacity(cfg.runs);
        let mut r_times = Vec::with_capacity(cfg.runs);
        let mut b_peak = 0u64;
        let mut r_peak = 0u64;

        for i in 0..cfg.runs {
            let mut b_cmd = Command::new(&base_dm);
            b_cmd.current_dir(&base_dir).args(&extra_args);
            let b_write = cfg.tmp_root.join(format!("exec-base-{i}"));
            if needs_write {
                let _ = fs::remove_dir_all(&b_write);
                fs::create_dir_all(&b_write)
                    .map_err(|e| format!("mkdir {}: {e}", b_write.display()))?;
                b_cmd.arg("--writePath").arg(&b_write);
            }
            let (dt, out) = run_capture(&mut b_cmd)?;
            b_times.push(dt);
            b_peak = b_peak.max(parse_peak_mb(&out));
            if needs_write {
                let _ = fs::remove_dir_all(&b_write);
            }

            let mut r_cmd = Command::new(&rust_dm);
            r_cmd.current_dir(&rust_dir).args(&extra_args);
            let r_write = cfg.tmp_root.join(format!("exec-rust-{i}"));
            if needs_write {
                let _ = fs::remove_dir_all(&r_write);
                fs::create_dir_all(&r_write)
                    .map_err(|e| format!("mkdir {}: {e}", r_write.display()))?;
                r_cmd.arg("--writePath").arg(&r_write);
            }
            let (dt, out) = run_capture(&mut r_cmd)?;
            r_times.push(dt);
            r_peak = r_peak.max(parse_peak_mb(&out));
            if needs_write {
                let _ = fs::remove_dir_all(&r_write);
            }
        }

        let b_med = median(b_times.clone());
        let r_med = median(r_times.clone());
        println!(
            "| `{label}` (wall) | `{b_med:.2}s` | `{r_med:.2}s` | `{}` |",
            format_delta(b_med, r_med)
        );
        println!(
            "| `{label}` (peak RSS) | `{b_peak} MB` | `{r_peak} MB` | `{}` |",
            format_delta(b_peak as f64, r_peak as f64)
        );
    }

    Ok(())
}

struct CleanBuildResult {
    gn_s: f64,
    rust_s: f64,
    cpp_s: f64,
    incr_cpp_s: Option<f64>,
}

impl CleanBuildResult {
    fn total(&self) -> f64 {
        self.gn_s + self.rust_s + self.cpp_s
    }
}

fn run_clean_build(
    cfg: &Config,
    src_dir: &Path,
    tag: &str,
    target: &str,
    is_rust: bool,
    incr_cpp_rel: Option<&Path>,
) -> Result<CleanBuildResult, String> {
    let tmp_dir = cfg.tmp_root.join(format!("clean-{tag}"));
    setup_isolated_tree(src_dir, &tmp_dir, incr_cpp_rel)?;

    let gn_arg = format!("--args={}", cfg.gn_args);
    let gn_s = run_quiet(
        Command::new("./bin/gn")
            .current_dir(&tmp_dir)
            .args(["gen", "out/Bench", &gn_arg]),
    )?;

    let rust_s = if is_rust {
        run_quiet(
            Command::new("./third_party/ninja/ninja")
                .current_dir(&tmp_dir)
                .args(["-C", "out/Bench", "skia_rs_ffi"]),
        )?
    } else {
        0.0
    };

    let cpp_s = run_quiet(
        Command::new("./third_party/ninja/ninja")
            .current_dir(&tmp_dir)
            .args(["-C", "out/Bench", target]),
    )?;

    let incr_cpp_s = if let Some(rel) = incr_cpp_rel {
        touch_file(&tmp_dir.join(rel))?;
        Some(run_quiet(
            Command::new("./third_party/ninja/ninja")
                .current_dir(&tmp_dir)
                .args(["-C", "out/Bench", target]),
        )?)
    } else {
        None
    };

    let _ = fs::remove_dir_all(&tmp_dir);
    Ok(CleanBuildResult {
        gn_s,
        rust_s,
        cpp_s,
        incr_cpp_s,
    })
}

fn bench_compile(cfg: &Config) -> Result<(), String> {
    let base_dir = cfg.repo_root.join("skia-baseline");
    let rust_dir = cfg.repo_root.join("skia-rewrite");

    println!("\n### Compile Time\n");
    println!("| Build Measurement | Baseline C++ | Rust Rewrite | Delta |");
    println!("|---|---:|---:|---:|");

    if cfg.run_clean_libskia {
        let b = run_clean_build(cfg, &base_dir, "libskia-base", "libskia.a", false, None)?;
        let r = run_clean_build(cfg, &rust_dir, "libskia-rust", "libskia.a", true, None)?;
        println!(
            "| Clean `libskia.a` | `{:.2}s` *(gn {:.2}s + C++ {:.2}s)* | `{:.2}s` *(gn {:.2}s + Rust {:.2}s + C++ {:.2}s)* | `{}` |",
            b.total(),
            b.gn_s,
            b.cpp_s,
            r.total(),
            r.gn_s,
            r.rust_s,
            r.cpp_s,
            format_delta(b.total(), r.total())
        );
    }

    if cfg.run_clean_dm {
        let b = run_clean_build(
            cfg,
            &base_dir,
            "dm-base",
            "dm",
            false,
            Some(Path::new("src/core/SkMathPriv.cpp")),
        )?;
        let r = run_clean_build(
            cfg,
            &rust_dir,
            "dm-rust",
            "dm",
            true,
            Some(Path::new("src/rust_bridge/SkMath_rust.cpp")),
        )?;
        println!(
            "| Clean full `dm` | `{:.2}s` *(gn {:.2}s + C++/link {:.2}s)* | `{:.2}s` *(gn {:.2}s + Rust {:.2}s + C++/link {:.2}s)* | `{}` |",
            b.total(),
            b.gn_s,
            b.cpp_s,
            r.total(),
            r.gn_s,
            r.rust_s,
            r.cpp_s,
            format_delta(b.total(), r.total())
        );
        if let (Some(bi), Some(ri)) = (b.incr_cpp_s, r.incr_cpp_s) {
            println!(
                "| Incremental `dm` (single `.cpp` touch) | `{bi:.2}s` | `{ri:.2}s` | `{}` |",
                format_delta(bi, ri)
            );
        }
    }

    if cfg.run_incr {
        let cpp_path = rust_dir.join("src/rust_bridge/SkMath_rust.cpp");
        let root_rs = rust_dir.join("rust/crates/skia-rs-math/src/lib.rs");
        let leaf_rs = rust_dir.join("rust/crates/skia-rs-canvas/src/lib.rs");

        if cpp_path.exists() {
            touch_file(&cpp_path)?;
            let dt = run_quiet(
                Command::new("./third_party/ninja/ninja")
                    .current_dir(&rust_dir)
                    .args(["-C", "out/Rust", "dm"]),
            )?;
            println!("| Incremental `dm` (Rust bridge `.cpp` touch) | — | `{dt:.2}s` | — |");
        }
        if root_rs.exists() {
            touch_file(&root_rs)?;
            let dt = run_quiet(
                Command::new("./third_party/ninja/ninja")
                    .current_dir(&rust_dir)
                    .args(["-C", "out/Rust", "dm"]),
            )?;
            println!("| Incremental `dm` (root `.rs` touch) | — | `{dt:.2}s` | — |");
        }
        if leaf_rs.exists() {
            touch_file(&leaf_rs)?;
            let dt = run_quiet(
                Command::new("./third_party/ninja/ninja")
                    .current_dir(&rust_dir)
                    .args(["-C", "out/Rust", "dm"]),
            )?;
            println!("| Incremental `dm` (leaf `.rs` touch) | — | `{dt:.2}s` | — |");
        }
    }

    Ok(())
}

fn main() -> ExitCode {
    let cfg = match parse_args() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ERROR: {e}");
            return ExitCode::FAILURE;
        }
    };

    if cfg.run_sizes {
        if let Err(e) = bench_sizes(&cfg) {
            eprintln!("ERROR (sizes): {e}");
            return ExitCode::FAILURE;
        }
    }
    if cfg.run_exec {
        if let Err(e) = bench_exec(&cfg) {
            eprintln!("ERROR (exec): {e}");
            return ExitCode::FAILURE;
        }
    }
    if cfg.run_clean_libskia || cfg.run_clean_dm || cfg.run_incr {
        if let Err(e) = bench_compile(&cfg) {
            eprintln!("ERROR (compile): {e}");
            return ExitCode::FAILURE;
        }
    }

    let _ = fs::remove_dir_all(&cfg.tmp_root);
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_median_and_delta() {
        assert!((median(vec![8.4, 7.38, 7.40]) - 7.40).abs() < 1e-9);
        assert!((median(vec![1.0, 3.0, 2.0, 4.0]) - 2.5).abs() < 1e-9);
        assert_eq!(format_delta(100.0, 94.1), "-5.9%");
        assert_eq!(format_delta(100.0, 104.6), "+4.6%");
    }

    #[test]
    fn test_parse_peak_mb() {
        let sample = "[3726/3726] 899MB RAM, 1249MB peak, 0 queued, 1 threads:\nFinished!\n";
        assert_eq!(parse_peak_mb(sample), 1249);
    }
}
