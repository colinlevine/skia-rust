#![forbid(unsafe_code)]

//! Safe Rust verification harness for `skia-rust`.
//!
//! Verifies:
//! 1. `skia-baseline/` has never changed since the rename commit
//!    (`88e8216d2ccddd50727971ac52939d5dc4feaf75`) and has no modified/untracked files.
//! 2. `skia-rewrite/{gm,tests,dm,bench,include}` are byte-for-byte identical to
//!    `skia-baseline/{gm,tests,dm,bench,include}`.
//! 3. `skia-rewrite/out/Rust/dm` passes with flags from `BASELINE.toml` and its
//!    `dm.json` MD5 hashes match `skia-baseline/out/dm/dm.json` 100%.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const BASELINE_RENAME_COMMIT: &str = "88e8216d2ccddd50727971ac52939d5dc4feaf75";
const PROTECTED_SUBDIRS: &[&str] = &["gm", "tests", "dm", "bench", "include"];

struct Args {
    repo_root: PathBuf,
    dm_bin: String,
    write_path: String,
    baseline_json: Option<PathBuf>,
    skip_protected_check: bool,
    match_filter: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let default_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let mut args = Args {
        repo_root: default_root,
        dm_bin: "out/Rust/dm".to_string(),
        write_path: "out/dm-rust".to_string(),
        baseline_json: None,
        skip_protected_check: false,
        match_filter: None,
    };

    let mut it = env::args().skip(1);
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--repo-root" => {
                args.repo_root = PathBuf::from(it.next().ok_or("--repo-root requires a value")?);
            }
            "--dm-bin" => {
                args.dm_bin = it.next().ok_or("--dm-bin requires a value")?;
            }
            "--write-path" => {
                args.write_path = it.next().ok_or("--write-path requires a value")?;
            }
            "--baseline-json" => {
                args.baseline_json =
                    Some(PathBuf::from(it.next().ok_or("--baseline-json requires a value")?));
            }
            "--skip-protected-check" => {
                args.skip_protected_check = true;
            }
            "--match" => {
                args.match_filter = Some(it.next().ok_or("--match requires a value")?);
            }
            "--help" | "-h" => {
                println!(
                    "Usage: verify-dm [--repo-root PATH] [--dm-bin REL] [--write-path REL] \
                     [--baseline-json PATH] [--match PATTERN] [--skip-protected-check]"
                );
                std::process::exit(0);
            }
            other => return Err(format!("Unknown argument: {other}")),
        }
    }
    Ok(args)
}

fn compare_dirs(dir_a: &Path, dir_b: &Path, rel: &Path, diffs: &mut Vec<String>) {
    let read_entries = |dir: &Path| -> Result<BTreeMap<String, fs::DirEntry>, String> {
        let mut map = BTreeMap::new();
        let iter = fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))?;
        for entry in iter {
            let entry = entry.map_err(|e| format!("dir entry {}: {e}", dir.display()))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            map.insert(name, entry);
        }
        Ok(map)
    };

    let map_a = match read_entries(dir_a) {
        Ok(m) => m,
        Err(e) => {
            diffs.push(e);
            return;
        }
    };
    let map_b = match read_entries(dir_b) {
        Ok(m) => m,
        Err(e) => {
            diffs.push(e);
            return;
        }
    };

    for name in map_a.keys() {
        if !map_b.contains_key(name) {
            diffs.push(format!("only in skia-baseline: {}", rel.join(name).display()));
        }
    }
    for name in map_b.keys() {
        if !map_a.contains_key(name) {
            diffs.push(format!("only in skia-rewrite:  {}", rel.join(name).display()));
        }
    }

    for (name, ent_a) in &map_a {
        let Some(ent_b) = map_b.get(name) else {
            continue;
        };
        let sub_rel = rel.join(name);
        let path_a = ent_a.path();
        let path_b = ent_b.path();
        let Ok(meta_a) = fs::symlink_metadata(&path_a) else {
            diffs.push(format!("stat failed: {}", path_a.display()));
            continue;
        };
        let Ok(meta_b) = fs::symlink_metadata(&path_b) else {
            diffs.push(format!("stat failed: {}", path_b.display()));
            continue;
        };

        if meta_a.is_dir() && meta_b.is_dir() {
            compare_dirs(&path_a, &path_b, &sub_rel, diffs);
        } else if meta_a.is_file() && meta_b.is_file() {
            if meta_a.len() != meta_b.len() {
                diffs.push(format!("size mismatch: {}", sub_rel.display()));
            } else {
                match (fs::read(&path_a), fs::read(&path_b)) {
                    (Ok(bytes_a), Ok(bytes_b)) if bytes_a == bytes_b => {}
                    (Ok(_), Ok(_)) => diffs.push(format!("content mismatch: {}", sub_rel.display())),
                    _ => diffs.push(format!("read error: {}", sub_rel.display())),
                }
            }
        } else if meta_a.file_type().is_symlink() && meta_b.file_type().is_symlink() {
            if fs::read_link(&path_a).ok() != fs::read_link(&path_b).ok() {
                diffs.push(format!("symlink mismatch: {}", sub_rel.display()));
            }
        } else {
            diffs.push(format!("file type mismatch: {}", sub_rel.display()));
        }
    }
}

fn check_protected_paths(repo_root: &Path) -> Result<(), String> {
    let diff_status = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["diff", "--quiet", BASELINE_RENAME_COMMIT, "--", "skia-baseline"])
        .status()
        .map_err(|e| format!("failed to run git diff: {e}"))?;

    if !diff_status.success() {
        let stat = Command::new("git")
            .arg("-C")
            .arg(repo_root)
            .args(["diff", "--stat", BASELINE_RENAME_COMMIT, "--", "skia-baseline"])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();
        return Err(format!(
            "skia-baseline/ has been modified since rename commit:\n{stat}"
        ));
    }

    let porcelain = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["status", "--porcelain", "--", "skia-baseline"])
        .output()
        .map_err(|e| format!("failed to run git status: {e}"))?;

    let status_out = String::from_utf8_lossy(&porcelain.stdout);
    if !status_out.trim().is_empty() {
        return Err(format!(
            "skia-baseline/ has modified or untracked files:\n{status_out}"
        ));
    }

    for sub in PROTECTED_SUBDIRS {
        let base_dir = repo_root.join("skia-baseline").join(sub);
        let rew_dir = repo_root.join("skia-rewrite").join(sub);
        let mut diffs = Vec::new();
        compare_dirs(&base_dir, &rew_dir, Path::new(sub), &mut diffs);
        if !diffs.is_empty() {
            let preview: Vec<String> = diffs.into_iter().take(20).collect();
            return Err(format!(
                "skia-rewrite/{sub} differs from skia-baseline/{sub}:\n  {}",
                preview.join("\n  ")
            ));
        }
    }

    Ok(())
}

/// Read `dm_flags` string value from `BASELINE.toml`.
fn read_dm_flags(baseline_toml: &Path) -> Result<String, String> {
    let text = fs::read_to_string(baseline_toml)
        .map_err(|e| format!("failed to read {}: {e}", baseline_toml.display()))?;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.starts_with('#') || !line.starts_with("dm_flags") {
            continue;
        }
        let Some((_, rhs)) = line.split_once('=') else {
            continue;
        };
        let rhs = rhs.trim();
        if let Some(rest) = rhs.strip_prefix('"') {
            if let Some(end) = rest.find('"') {
                return Ok(rest[..end].to_string());
            }
        }
    }
    Err(format!("dm_flags not found in {}", baseline_toml.display()))
}

/// Minimal strict JSON parser for Skia `dm.json` (`results[*].{key.{config,source_type,name}, md5}`).
enum JsonVal {
    Null,
    Bool,
    Num,
    Str(String),
    Arr(Vec<JsonVal>),
    Obj(BTreeMap<String, JsonVal>),
}

struct JsonParser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> JsonParser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            bytes: input.as_bytes(),
            pos: 0,
        }
    }

    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_ws();
        self.bytes.get(self.pos).copied()
    }

    fn parse_val(&mut self) -> Result<JsonVal, String> {
        match self.peek() {
            Some(b'{') => self.parse_obj(),
            Some(b'[') => self.parse_arr(),
            Some(b'"') => self.parse_str().map(JsonVal::Str),
            Some(b't') => {
                self.expect_lit(b"true")?;
                Ok(JsonVal::Bool)
            }
            Some(b'f') => {
                self.expect_lit(b"false")?;
                Ok(JsonVal::Bool)
            }
            Some(b'n') => {
                self.expect_lit(b"null")?;
                Ok(JsonVal::Null)
            }
            Some(b'-' | b'0'..=b'9') => {
                self.skip_num();
                Ok(JsonVal::Num)
            }
            other => Err(format!("unexpected JSON byte {other:?} at {}", self.pos)),
        }
    }

    fn expect_lit(&mut self, lit: &[u8]) -> Result<(), String> {
        self.skip_ws();
        if self.bytes.get(self.pos..self.pos + lit.len()) == Some(lit) {
            self.pos += lit.len();
            Ok(())
        } else {
            Err(format!("expected literal at {}", self.pos))
        }
    }

    fn skip_num(&mut self) {
        self.skip_ws();
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E' => self.pos += 1,
                _ => break,
            }
        }
    }

    fn parse_str(&mut self) -> Result<String, String> {
        self.skip_ws();
        if self.bytes.get(self.pos) != Some(&b'"') {
            return Err(format!("expected '\"' at {}", self.pos));
        }
        self.pos += 1;
        let mut out = String::new();
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            self.pos += 1;
            match b {
                b'"' => return Ok(out),
                b'\\' => {
                    let esc = *self
                        .bytes
                        .get(self.pos)
                        .ok_or_else(|| "unterminated escape".to_string())?;
                    self.pos += 1;
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000C}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hex = std::str::from_utf8(
                                self.bytes
                                    .get(self.pos..self.pos + 4)
                                    .ok_or_else(|| "short \\u escape".to_string())?,
                            )
                            .map_err(|e| e.to_string())?;
                            self.pos += 4;
                            let code =
                                u32::from_str_radix(hex, 16).map_err(|e| e.to_string())?;
                            if let Some(ch) = char::from_u32(code) {
                                out.push(ch);
                            }
                        }
                        other => return Err(format!("bad escape byte {other}")),
                    }
                }
                _ => out.push(b as char),
            }
        }
        Err("unterminated string".to_string())
    }

    fn parse_arr(&mut self) -> Result<JsonVal, String> {
        self.skip_ws();
        self.pos += 1; // '['
        let mut items = Vec::new();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(JsonVal::Arr(items));
        }
        loop {
            items.push(self.parse_val()?);
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(JsonVal::Arr(items));
                }
                other => return Err(format!("expected ',' or ']' got {other:?} at {}", self.pos)),
            }
        }
    }

    fn parse_obj(&mut self) -> Result<JsonVal, String> {
        self.skip_ws();
        self.pos += 1; // '{'
        let mut map = BTreeMap::new();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(JsonVal::Obj(map));
        }
        loop {
            let k = self.parse_str()?;
            if self.peek() != Some(b':') {
                return Err(format!("expected ':' at {}", self.pos));
            }
            self.pos += 1;
            let v = self.parse_val()?;
            map.insert(k, v);
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(JsonVal::Obj(map));
                }
                other => return Err(format!("expected ',' or '}}' got {other:?} at {}", self.pos)),
            }
        }
    }
}

type DmKey = (String, String, String);

fn get_str<'a>(obj: &'a BTreeMap<String, JsonVal>, key: &str) -> Option<&'a str> {
    match obj.get(key) {
        Some(JsonVal::Str(s)) => Some(s.as_str()),
        _ => None,
    }
}

fn load_dm_hashes(path: &Path) -> Result<BTreeMap<DmKey, String>, String> {
    let text = fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let root = JsonParser::new(&text).parse_val()?;
    let JsonVal::Obj(root_obj) = root else {
        return Err("dm.json root is not an object".to_string());
    };
    let Some(JsonVal::Arr(results)) = root_obj.get("results") else {
        return Err("dm.json missing 'results' array".to_string());
    };
    let mut map = BTreeMap::new();
    for entry in results {
        let JsonVal::Obj(obj) = entry else { continue };
        let Some(JsonVal::Obj(key_obj)) = obj.get("key") else { continue };
        let Some(md5) = get_str(obj, "md5") else { continue };
        let cfg = get_str(key_obj, "config").unwrap_or("").to_string();
        let src = get_str(key_obj, "source_type").unwrap_or("").to_string();
        let name = get_str(key_obj, "name").unwrap_or("").to_string();
        map.insert((cfg, src, name), md5.to_string());
    }
    Ok(map)
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let repo_root = fs::canonicalize(&args.repo_root)
        .map_err(|e| format!("invalid repo root {}: {e}", args.repo_root.display()))?;
    let rewrite_dir = repo_root.join("skia-rewrite");

    if !args.skip_protected_check {
        check_protected_paths(&repo_root)?;
    }

    let raw_flags = read_dm_flags(&repo_root.join("BASELINE.toml"))?;
    let tokens: Vec<&str> = raw_flags.split_whitespace().collect();
    let mut dm_flags: Vec<String> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i] == "--writePath" || tokens[i] == "-w" {
            i += 2;
            continue;
        }
        dm_flags.push(tokens[i].to_string());
        i += 1;
    }

    let write_dir = rewrite_dir.join(&args.write_path);
    if write_dir.starts_with(repo_root.join("skia-baseline")) {
        return Err("--write-path must NEVER point inside skia-baseline/".to_string());
    }
    fs::create_dir_all(&write_dir)
        .map_err(|e| format!("failed to create {}: {e}", write_dir.display()))?;

    dm_flags.push("--writePath".to_string());
    dm_flags.push(args.write_path.clone());
    if let Some(m) = &args.match_filter {
        dm_flags.push("--match".to_string());
        dm_flags.push(m.clone());
    }

    let dm_bin = rewrite_dir.join(&args.dm_bin);
    println!("Running: {} {}", dm_bin.display(), dm_flags.join(" "));
    let status = Command::new(&dm_bin)
        .args(&dm_flags)
        .current_dir(&rewrite_dir)
        .status()
        .map_err(|e| format!("failed to execute {}: {e}", dm_bin.display()))?;
    if !status.success() {
        return Err(format!("DM exited with status {status}"));
    }

    let ref_json = args
        .baseline_json
        .unwrap_or_else(|| repo_root.join("skia-baseline/out/dm/dm.json"));
    let cur_json = write_dir.join("dm.json");
    let ref_map = load_dm_hashes(&ref_json)?;
    let cur_map = load_dm_hashes(&cur_json)?;

    if args.match_filter.is_some() {
        let mut matched = 0usize;
        let mut mismatched = Vec::new();
        for (k, actual) in &cur_map {
            if let Some(expected) = ref_map.get(k) {
                if expected == actual {
                    matched += 1;
                } else {
                    mismatched.push((k, expected, actual));
                }
            }
        }
        println!("Filtered DM: {matched}/{} match", cur_map.len());
        if !mismatched.is_empty() {
            for (k, expected, actual) in mismatched.iter().take(20) {
                eprintln!("  MISMATCH {k:?}: expected {expected}, got {actual}");
            }
            return Err(format!("{} filtered DM mismatches", mismatched.len()));
        }
        return Ok(());
    }

    let total = ref_map.len();
    let mut matched = 0usize;
    let mut missing = Vec::new();
    let mut mismatched = Vec::new();
    for (k, expected) in &ref_map {
        match cur_map.get(k) {
            None => missing.push(k),
            Some(actual) if actual == expected => matched += 1,
            Some(actual) => mismatched.push((k, expected, actual)),
        }
    }
    let ref_keys: BTreeSet<&DmKey> = ref_map.keys().collect();
    let extra: Vec<&DmKey> = cur_map.keys().filter(|k| !ref_keys.contains(k)).collect();

    println!("DM: {matched}/{total} match");
    if !missing.is_empty() {
        eprintln!("  MISSING ({}): {:?}", missing.len(), &missing[..missing.len().min(10)]);
    }
    if !extra.is_empty() {
        eprintln!("  EXTRA ({}): {:?}", extra.len(), &extra[..extra.len().min(10)]);
    }
    if !mismatched.is_empty() {
        eprintln!("  MISMATCHED ({}):", mismatched.len());
        for (k, expected, actual) in mismatched.iter().take(20) {
            eprintln!("    {k:?}: expected {expected}, got {actual}");
        }
    }

    if matched == total && missing.is_empty() && extra.is_empty() && mismatched.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "DM verification failed: {matched}/{total} match, {} missing, {} extra, {} mismatched",
            missing.len(),
            extra.len(),
            mismatched.len()
        ))
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ERROR: {e}");
            ExitCode::FAILURE
        }
    }
}
