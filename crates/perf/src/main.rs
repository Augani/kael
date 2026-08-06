#![doc = include_str!("../README.md")]

mod implementation;

use implementation::{FailKind, Importance, Output, TestMdata, Timings, consts};
use serde::Deserialize;

use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    num::NonZero,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

/// How many iterations to attempt the first time a test is run.
const DEFAULT_ITER_COUNT: NonZero<usize> = NonZero::new(3).unwrap();
/// Multiplier for the iteration count when a test doesn't pass the noise cutoff.
const ITER_COUNT_MUL: NonZero<usize> = NonZero::new(4).unwrap();
/// Largest accepted saved run, which prevents accidental unbounded reads.
const MAX_RUN_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// Largest accepted run identifier.
const MAX_RUN_IDENTIFIER_LEN: usize = 128;
/// Largest accepted test-list output from a benchmark binary.
const MAX_TEST_LIST_BYTES: usize = 16 * 1024 * 1024;
/// Largest accepted metadata output from one generated test.
const MAX_METADATA_BYTES: usize = 64 * 1024;
/// Largest accepted Hyperfine JSON response.
const MAX_HYPERFINE_OUTPUT_BYTES: usize = 1024 * 1024;
/// Largest number of paired benchmarks accepted from one test binary.
const MAX_PERF_TESTS: usize = 16 * 1024;
/// Largest accepted generated test name.
const MAX_TEST_NAME_BYTES: usize = 16 * 1024;

/// Do we keep stderr empty while running the tests?
static QUIET: AtomicBool = AtomicBool::new(false);

/// Report a failure into the output and skip an iteration.
macro_rules! fail {
    ($output:ident, $name:expr, $kind:expr) => {{
        $output.failure($name, None, None, $kind);
        continue;
    }};
    ($output:ident, $name:expr, $mdata:expr, $kind:expr) => {{
        $output.failure($name, Some($mdata), None, $kind);
        continue;
    }};
    ($output:ident, $name:expr, $mdata:expr, $count:expr, $kind:expr) => {{
        $output.failure($name, Some($mdata), Some($count), $kind);
        continue;
    }};
}

/// How does this perf run return its output?
enum OutputKind<'a> {
    /// Print markdown to the terminal.
    Markdown,
    /// Save JSON to a file.
    Json(&'a str),
}

impl OutputKind<'_> {
    /// Logs the output of a run as per the `OutputKind`.
    fn log(&self, output: &Output, t_bin: &str) -> Result<(), String> {
        match self {
            OutputKind::Markdown => {
                println!("{output}");
                Ok(())
            }
            OutputKind::Json(ident) => {
                validate_run_identifier(ident)?;
                let runs_dir = runs_directory(true)?;
                // Get the test binary's crate's name; a path like
                // target/release-fast/deps/kael-061ff76c9b7af5d7
                // would be reduced to just "kael".
                let test_bin_file_stem = Path::new(t_bin)
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| "test binary must have a UTF-8 file name".to_owned())?;
                let test_bin_stripped = strip_test_binary_hash(test_bin_file_stem);
                validate_run_component(test_bin_stripped, "test crate name")?;
                let file_path = runs_dir.join(format!("{ident}.{test_bin_stripped}.json"));
                let bytes = serde_json::to_vec(output)
                    .map_err(|error| format!("failed to encode performance run: {error}"))?;
                if bytes.len() as u64 > MAX_RUN_FILE_BYTES {
                    return Err(format!(
                        "performance run exceeds the {MAX_RUN_FILE_BYTES}-byte limit"
                    ));
                }
                refuse_symlink_target(&file_path)?;
                let mut options = OpenOptions::new();
                options.write(true).create(true).truncate(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt as _;
                    options.mode(0o600);
                }
                let mut out_file = options
                    .open(&file_path)
                    .map_err(|error| format!("failed to open {}: {error}", file_path.display()))?;
                out_file
                    .write_all(&bytes)
                    .and_then(|()| out_file.sync_all())
                    .map_err(|error| format!("failed to write {}: {error}", file_path.display()))?;
                if !QUIET.load(Ordering::Relaxed) {
                    eprintln!("JSON output written to {}", file_path.display());
                }
                Ok(())
            }
        }
    }
}

/// Ensures a run identifier is a bounded, path-safe file-name component.
fn validate_run_identifier(identifier: &str) -> Result<(), String> {
    if identifier.is_empty()
        || identifier.len() > MAX_RUN_IDENTIFIER_LEN
        || !identifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(format!(
            "invalid run identifier {identifier:?}; use 1-{MAX_RUN_IDENTIFIER_LEN} ASCII letters, digits, '-' or '_'"
        ));
    }
    Ok(())
}

/// Ensures an internally derived saved-run component is path-safe.
fn validate_run_component(component: &str, label: &str) -> Result<(), String> {
    if component.is_empty()
        || !component
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(format!("{label} contains unsupported characters"));
    }
    Ok(())
}

/// Removes Cargo's trailing hexadecimal artifact hash from a test binary name.
fn strip_test_binary_hash(file_stem: &str) -> &str {
    file_stem
        .rsplit_once('-')
        .map_or(file_stem, |(name, suffix)| {
            if suffix.len() >= 8 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                name
            } else {
                file_stem
            }
        })
}

/// Refuses to overwrite symlinks and other non-regular destinations.
fn refuse_symlink_target(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(format!(
            "refusing to replace a non-regular file: {}",
            path.display()
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("failed to inspect {}: {error}", path.display())),
    }
}

/// Resolves and validates the saved-run directory relative to the invocation directory.
fn runs_directory(create: bool) -> Result<PathBuf, String> {
    let directory = std::env::current_dir()
        .map(|directory| directory.join(consts::RUNS_DIR))
        .map_err(|error| format!("failed to resolve the current directory: {error}"))?;
    if create {
        fs::create_dir_all(&directory)
            .map_err(|error| format!("failed to create {}: {error}", directory.display()))?;
    }
    let metadata = fs::symlink_metadata(&directory)
        .map_err(|error| format!("failed to inspect {}: {error}", directory.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "saved-run path is not a real directory: {}",
            directory.display()
        ));
    }
    #[cfg(unix)]
    if create {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).map_err(|error| {
            format!(
                "failed to secure saved-run directory {}: {error}",
                directory.display()
            )
        })?;
    }
    Ok(directory)
}

/// Runs a child while capturing no more than `limit` bytes from stdout.
fn bounded_command_stdout(
    command: &mut Command,
    limit: usize,
    label: &str,
) -> Result<(std::process::ExitStatus, Vec<u8>), String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to start {label}: {error}"))?;
    let Some(stdout) = child.stdout.take() else {
        terminate_child(&mut child);
        return Err(format!("failed to capture {label} output"));
    };
    let mut bytes = Vec::new();
    let read_result = stdout
        .take(u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1))
        .read_to_end(&mut bytes);
    if let Err(error) = read_result {
        terminate_child(&mut child);
        return Err(format!("failed to read {label} output: {error}"));
    }
    if bytes.len() > limit {
        terminate_child(&mut child);
        return Err(format!("{label} output exceeds the {limit}-byte limit"));
    }
    let status = child
        .wait()
        .map_err(|error| format!("failed to wait for {label}: {error}"))?;
    Ok((status, bytes))
}

/// Best-effort cleanup for a child whose output contract was violated.
fn terminate_child(child: &mut std::process::Child) {
    drop(child.kill());
    drop(child.wait());
}

/// Runs a given metadata-returning function from a test handler, parsing its
/// output into a `TestMdata`.
fn parse_mdata(t_bin: &str, mdata_fn: &str) -> Result<TestMdata, FailKind> {
    let mut cmd = Command::new(t_bin);
    cmd.args([mdata_fn, "--exact", "--nocapture"]);
    let (status, stdout) = bounded_command_stdout(&mut cmd, MAX_METADATA_BYTES, "metadata test")
        .map_err(|_| FailKind::BadMetadata)?;
    if !status.success() {
        return Err(FailKind::BadMetadata);
    }
    let stdout = String::from_utf8_lossy(&stdout);
    parse_mdata_stdout(&stdout)
}

/// Parses the versioned metadata protocol emitted by a perf metadata test.
fn parse_mdata_stdout(stdout: &str) -> Result<TestMdata, FailKind> {
    let mut version = None;
    let mut iterations = None;
    let mut importance = Importance::default();
    let mut weight = consts::WEIGHT_DEFAULT;
    let mut importance_set = false;
    let mut weight_set = false;
    for line in stdout
        .lines()
        .filter_map(|l| l.strip_prefix(consts::MDATA_LINE_PREF))
    {
        let mut items = line.split_whitespace();
        // For v0, the identifier comes first, followed by exactly one value.
        let identifier = items.next().ok_or(FailKind::BadMetadata)?;
        let value = items.next().ok_or(FailKind::BadMetadata)?;
        if items.next().is_some() {
            return Err(FailKind::BadMetadata);
        }
        match identifier {
            consts::VERSION_LINE_NAME => {
                if version.is_some() {
                    return Err(FailKind::BadMetadata);
                }
                let v = value.parse::<u32>().map_err(|_| FailKind::BadMetadata)?;
                if v > consts::MDATA_VER {
                    return Err(FailKind::VersionMismatch);
                }
                version = Some(v);
            }
            consts::ITER_COUNT_LINE_NAME => {
                if iterations.is_some() {
                    return Err(FailKind::BadMetadata);
                }
                // This should never be zero!
                iterations = Some(
                    value
                        .parse::<usize>()
                        .map_err(|_| FailKind::BadMetadata)?
                        .try_into()
                        .map_err(|_| FailKind::BadMetadata)?,
                );
            }
            consts::IMPORTANCE_LINE_NAME => {
                if importance_set {
                    return Err(FailKind::BadMetadata);
                }
                importance_set = true;
                importance = match value {
                    "critical" => Importance::Critical,
                    "important" => Importance::Important,
                    "average" => Importance::Average,
                    "iffy" => Importance::Iffy,
                    "fluff" => Importance::Fluff,
                    _ => return Err(FailKind::BadMetadata),
                };
            }
            consts::WEIGHT_LINE_NAME => {
                if weight_set {
                    return Err(FailKind::BadMetadata);
                }
                weight_set = true;
                weight = value.parse::<u8>().map_err(|_| FailKind::BadMetadata)?;
                if weight == 0 {
                    return Err(FailKind::BadMetadata);
                }
            }
            _ => return Err(FailKind::BadMetadata),
        }
    }

    Ok(TestMdata {
        version: version.ok_or(FailKind::BadMetadata)?,
        // Iterations may be determined by us and thus left unspecified.
        iterations,
        // In principle this should always be set, but just for the sake of
        // stability allow the potentially-breaking change of not reporting the
        // importance without erroring. Maybe we want to change this.
        importance,
        // Same with weight.
        weight,
    })
}

/// Parsed arguments for comparing two saved runs.
struct CompareArgs<'a> {
    /// Optional Markdown report destination.
    save_to: Option<&'a Path>,
    /// Identifier of the newer run.
    new: &'a str,
    /// Identifier of the baseline run.
    old: &'a str,
}

/// Parses the strict comparison command-line contract.
fn parse_compare_args(args: &[String]) -> Result<CompareArgs<'_>, String> {
    let (save_to, identifiers) = match args.first() {
        Some(argument) if argument.starts_with("--save") => {
            let destination = argument.strip_prefix("--save=").ok_or_else(|| {
                "--save must include a destination, for example --save=report.md".to_owned()
            })?;
            if destination.is_empty() {
                return Err("--save destination cannot be empty".to_owned());
            }
            (Some(Path::new(destination)), &args[1..])
        }
        _ => (None, args),
    };
    let [new, old] = identifiers else {
        return Err("compare expects exactly NEW_RUN and BASELINE_RUN identifiers".to_owned());
    };
    validate_run_identifier(new)?;
    validate_run_identifier(old)?;
    if new == old {
        return Err("new and baseline run identifiers must differ".to_owned());
    }
    Ok(CompareArgs { save_to, new, old })
}

/// Extracts the crate component from an exact `<run>.<crate>.json` file name.
fn run_file_component<'a>(file_name: &'a str, identifier: &str) -> Option<&'a str> {
    let component = file_name
        .strip_prefix(identifier)?
        .strip_prefix('.')?
        .strip_suffix(".json")?;
    validate_run_component(component, "saved run crate name")
        .is_ok()
        .then_some(component)
}

/// Reads a bounded, regular saved-run file.
fn read_saved_output(path: &Path) -> Result<Output, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "saved run is not a regular file: {}",
            path.display()
        ));
    }
    if metadata.len() > MAX_RUN_FILE_BYTES {
        return Err(format!(
            "saved run exceeds the {MAX_RUN_FILE_BYTES}-byte limit: {}",
            path.display()
        ));
    }
    let file = fs::File::open(path)
        .map_err(|error| format!("failed to open {}: {error}", path.display()))?;
    let capacity = usize::try_from(metadata.len())
        .map_err(|_| format!("saved run is too large for this target: {}", path.display()))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .map_err(|error| format!("failed to allocate saved-run buffer: {error}"))?;
    file.take(MAX_RUN_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    if bytes.len() as u64 > MAX_RUN_FILE_BYTES {
        return Err(format!(
            "saved run grew beyond the {MAX_RUN_FILE_BYTES}-byte limit: {}",
            path.display()
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))
}

/// Compares the perf results of two profiles as per the arguments passed in.
fn compare_profiles(args: &[String]) -> Result<(), String> {
    let args = parse_compare_args(args)?;
    let runs_dir = runs_directory(false)?;

    // Use the blank outputs initially, so we can merge into these with prefixes.
    let mut outputs_new = Output::blank();
    let mut outputs_old = Output::blank();
    let mut new_files = 0usize;
    let mut old_files = 0usize;

    let entries = runs_dir
        .read_dir()
        .map_err(|error| format!("failed to read {}: {error}", runs_dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!("failed to read an entry in {}: {error}", runs_dir.display())
        })?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if let Some(component) = run_file_component(&name, args.old) {
            outputs_old.merge(read_saved_output(&entry.path())?, component);
            old_files += 1;
        } else if let Some(component) = run_file_component(&name, args.new) {
            outputs_new.merge(read_saved_output(&entry.path())?, component);
            new_files += 1;
        }
    }
    if new_files == 0 {
        return Err(format!("no saved runs found for {:?}", args.new));
    }
    if old_files == 0 {
        return Err(format!("no saved runs found for {:?}", args.old));
    }

    let res = outputs_new.compare_perf(outputs_old);
    if let Some(filename) = args.save_to {
        refuse_symlink_target(filename)?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(filename)
            .map_err(|error| format!("failed to open {}: {error}", filename.display()))?;
        file.write_all(format!("{res}").as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("failed to write {}: {error}", filename.display()))?;
    } else {
        println!("{res}");
    }
    Ok(())
}

/// Runs a test binary, filtering out tests which aren't marked for perf triage
/// and giving back the list of tests we care about.
///
/// The output pairs each generated test function with its metadata function.
fn get_tests(t_bin: &str) -> Result<Vec<(String, String)>, String> {
    let mut cmd = Command::new(t_bin);
    // --format=json is nightly-only :(
    cmd.args(["--list", "--format=terse"]);
    let (status, stdout) = bounded_command_stdout(&mut cmd, MAX_TEST_LIST_BYTES, "test discovery")?;
    if !status.success() {
        return Err(format!(
            "cannot profile test binary {t_bin:?}: discovery exited with {status}"
        ));
    }
    if !QUIET.load(Ordering::Relaxed) {
        eprintln!("Test binary ran successfully; starting profile...");
    }
    parse_test_list(&String::from_utf8_lossy(&stdout))
}

/// Parses and pairs generated performance tests from Rust's terse test listing.
fn parse_test_list(stdout: &str) -> Result<Vec<(String, String)>, String> {
    let mut tests = BTreeMap::<String, (Option<String>, Option<String>)>::new();
    for line in stdout.lines() {
        let Some((name, kind)) = line.rsplit_once(": ") else {
            continue;
        };
        if kind != "test" {
            continue;
        }
        if name.len() > MAX_TEST_NAME_BYTES {
            return Err(format!(
                "generated test name exceeds the {MAX_TEST_NAME_BYTES}-byte limit"
            ));
        }
        let (base, is_metadata) = if let Some(base) = name.strip_suffix(consts::SUF_NORMAL) {
            (base, false)
        } else if let Some(base) = name.strip_suffix(consts::SUF_MDATA) {
            (base, true)
        } else {
            continue;
        };
        if base.is_empty() {
            return Err("generated performance test has an empty base name".to_owned());
        }
        if !tests.contains_key(base) && tests.len() >= MAX_PERF_TESTS {
            return Err(format!(
                "test binary exceeds the {MAX_PERF_TESTS}-benchmark limit"
            ));
        }
        let pair = tests.entry(base.to_owned()).or_default();
        let slot = if is_metadata {
            &mut pair.1
        } else {
            &mut pair.0
        };
        if slot.replace(name.to_owned()).is_some() {
            return Err(format!("duplicate generated performance test {name:?}"));
        }
    }

    tests
        .into_iter()
        .map(|(base, (test, metadata))| match (test, metadata) {
            (Some(test), Some(metadata)) => Ok((test, metadata)),
            _ => Err(format!(
                "generated performance test {base:?} is missing its test or metadata pair"
            )),
        })
        .collect()
}

/// Runs the specified test `count` times, returning the time taken if the test
/// succeeded.
#[inline]
fn spawn_and_iterate(t_bin: &str, t_name: &str, count: NonZero<usize>) -> Option<Duration> {
    let mut cmd = Command::new(t_bin);
    cmd.args([t_name, "--exact"]);
    cmd.env(consts::ITER_ENV_VAR, format!("{count}"));
    // Don't let the child muck up our stdin/out/err.
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());
    let pre = Instant::now();
    // Discard the output beyond ensuring success.
    let out = cmd.spawn().ok()?.wait();
    let post = Instant::now();
    out.iter().find_map(|s| s.success().then_some(post - pre))
}

/// Triage a test to determine the correct number of iterations that it should run.
/// Specifically, repeatedly runs the given test until its execution time exceeds
/// `thresh`, calling `step(iterations)` after every failed run to determine the new
/// iteration count. Returns `None` if the test errored or `step` returned `None`,
/// else `Some(iterations)`.
///
fn triage_test(
    t_bin: &str,
    t_name: &str,
    thresh: Duration,
    mut step: impl FnMut(NonZero<usize>) -> Option<NonZero<usize>>,
) -> Option<NonZero<usize>> {
    let mut iter_count = DEFAULT_ITER_COUNT;
    // It's possible that the first loop of a test might be an outlier (e.g. it's
    // doing some caching), in which case we want to skip it.
    let duration_once = spawn_and_iterate(t_bin, t_name, NonZero::new(1).unwrap())?;
    loop {
        let duration = spawn_and_iterate(t_bin, t_name, iter_count)?;
        if duration.saturating_sub(duration_once) > thresh {
            break Some(iter_count);
        }
        let new = step(iter_count)?;
        if new <= iter_count {
            return None;
        }
        iter_count = new;
    }
}

/// Minimal subset of Hyperfine's JSON export envelope.
#[derive(Deserialize)]
struct HyperfineOutput {
    /// Results emitted for each benchmark command.
    results: Vec<HyperfineResult>,
}

/// Minimal timing fields from one Hyperfine JSON result.
#[derive(Deserialize)]
struct HyperfineResult {
    /// Mean duration in seconds.
    mean: f64,
    /// Standard deviation in seconds.
    stddev: f64,
}

/// Parses one valid timing record from a Hyperfine JSON export.
fn parse_hyperfine_timings(bytes: &[u8]) -> Option<Timings> {
    let parsed: HyperfineOutput = serde_json::from_slice(bytes).ok()?;
    let [result] = parsed.results.as_slice() else {
        return None;
    };
    if !result.mean.is_finite()
        || result.mean <= 0.0
        || !result.stddev.is_finite()
        || result.stddev < 0.0
    {
        return None;
    }
    Some(Timings {
        mean: Duration::from_secs_f64(result.mean),
        stddev: Duration::from_secs_f64(result.stddev),
    })
}

/// Quotes one argument for Hyperfine's shell-free command parser.
fn hyperfine_argument(value: &str) -> Option<String> {
    if value.chars().any(char::is_control) {
        return None;
    }
    #[cfg(target_os = "windows")]
    let value = value.replace('\\', "/");
    #[cfg(not(target_os = "windows"))]
    let value = value.to_owned();
    Some(format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

/// Builds the one command value accepted by Hyperfine without invoking a shell.
fn hyperfine_command(t_bin: &str, t_name: &str) -> Option<String> {
    Some(format!(
        "{} --exact {}",
        hyperfine_argument(t_bin)?,
        hyperfine_argument(t_name)?
    ))
}

/// Profiles a given test with Hyperfine, returning the mean and standard deviation
/// for its runtime. If the test or profiler errors, returns `None` instead.
fn hyp_profile(t_bin: &str, t_name: &str, iterations: NonZero<usize>) -> Option<Timings> {
    let mut perf_cmd = Command::new("hyperfine");
    // Avoid an intermediate shell so repository paths cannot become shell syntax.
    // Hyperfine still accepts the executable and its arguments as one command value.
    let command = hyperfine_command(t_bin, t_name)?;
    perf_cmd.args([
        "--style",
        "none",
        "--warmup",
        "1",
        "--export-json",
        "-",
        "--shell=none",
        &command,
    ]);
    perf_cmd.env(consts::ITER_ENV_VAR, format!("{iterations}"));
    let (status, stdout) = bounded_command_stdout(
        &mut perf_cmd,
        MAX_HYPERFINE_OUTPUT_BYTES,
        "Hyperfine profiler",
    )
    .ok()?;
    if !status.success() {
        return None;
    }

    parse_hyperfine_timings(&stdout)
}

/// Runs the command-line application and returns user-facing failures.
fn run() -> Result<(), String> {
    let args = std::env::args().collect::<Vec<_>>();
    // We get passed the test we need to run as the 1st argument after our own name.
    let t_bin = args
        .get(1)
        .ok_or_else(|| "expected a test binary path or the 'compare' command".to_owned())?;

    // We're being asked to compare two results, not run the profiler.
    if t_bin == "compare" {
        return compare_profiles(&args[2..]);
    }

    // Minimum test importance we care about this run.
    let mut thresh = Importance::Iffy;
    // Where to print the output of this run.
    let mut out_kind = OutputKind::Markdown;

    for arg in args.iter().skip(2) {
        match arg.as_str() {
            "--critical" => thresh = Importance::Critical,
            "--important" => thresh = Importance::Important,
            "--average" => thresh = Importance::Average,
            "--iffy" => thresh = Importance::Iffy,
            "--fluff" => thresh = Importance::Fluff,
            "--quiet" => QUIET.store(true, Ordering::Relaxed),
            s if s.starts_with("--json") => {
                let identifier = s
                    .strip_prefix("--json=")
                    .ok_or_else(|| "invalid JSON option; pass --json=RUN_IDENTIFIER".to_owned())?;
                validate_run_identifier(identifier)?;
                out_kind = OutputKind::Json(identifier);
            }
            unknown => return Err(format!("unknown option: {unknown}")),
        }
    }
    if !QUIET.load(Ordering::Relaxed) {
        eprintln!("Starting perf check");
    }

    let mut output = Output::default();

    // Spawn and profile an instance of each perf-sensitive test, via hyperfine.
    // Each test is a pair of (test, metadata-returning-fn), so grab both. We also
    // know the list is sorted.
    let tests = get_tests(t_bin)?;
    let len = tests.len();
    for (idx, (t_name, t_mdata)) in tests.into_iter().enumerate() {
        if !QUIET.load(Ordering::Relaxed) {
            eprint!("\rProfiling test {}/{}", idx + 1, len);
        }
        // Pretty-printable stripped name for the test.
        let t_name_pretty = t_name
            .strip_suffix(consts::SUF_NORMAL)
            .unwrap_or(&t_name)
            .to_owned();

        // Get the metadata this test reports for us.
        let t_mdata = match parse_mdata(t_bin, &t_mdata) {
            Ok(mdata) => mdata,
            Err(err) => fail!(output, t_name_pretty, err),
        };

        if t_mdata.importance < thresh {
            fail!(output, t_name_pretty, t_mdata, FailKind::Skipped);
        }

        // Time test execution to see how many iterations we need to do in order
        // to account for random noise. This is skipped for tests with fixed
        // iteration counts.
        let final_iter_count = t_mdata.iterations.or_else(|| {
            triage_test(t_bin, &t_name, consts::NOISE_CUTOFF, |c| {
                if let Some(c) = c.checked_mul(ITER_COUNT_MUL) {
                    Some(c)
                } else {
                    // This should almost never happen, but maybe..?
                    eprintln!(
                        "WARNING: Ran nearly usize::MAX iterations of test {t_name_pretty}; skipping"
                    );
                    None
                }
            })
        });

        // Don't profile failing tests.
        let Some(final_iter_count) = final_iter_count else {
            fail!(output, t_name_pretty, t_mdata, FailKind::Triage);
        };

        // Now profile!
        if let Some(timings) = hyp_profile(t_bin, &t_name, final_iter_count) {
            output.success(t_name_pretty, t_mdata, final_iter_count, timings);
        } else {
            fail!(
                output,
                t_name_pretty,
                t_mdata,
                final_iter_count,
                FailKind::Profile
            );
        }
    }
    if !QUIET.load(Ordering::Relaxed) {
        if output.is_empty() {
            eprintln!("Nothing to do.");
        } else {
            // If stdout and stderr are on the same terminal, move us after the
            // output from above.
            eprintln!();
        }
    }

    // No need making an empty json file on every empty test bin.
    if output.is_empty() {
        return Ok(());
    }

    out_kind.log(&output, t_bin)
}

fn main() {
    if let Err(error) = run() {
        eprintln!("kael_perf: {error}");
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_parser_accepts_a_complete_version_zero_record() {
        let metadata = parse_mdata_stdout(
            "ignored harness output\n\
             KAEL_MDATA_version 0\n\
             KAEL_MDATA_iter_count 12\n\
             KAEL_MDATA_importance critical\n\
             KAEL_MDATA_weight 75\n",
        )
        .unwrap();

        assert_eq!(metadata.version, 0);
        assert_eq!(metadata.iterations, NonZero::new(12));
        assert_eq!(metadata.importance, Importance::Critical);
        assert_eq!(metadata.weight, 75);
    }

    #[test]
    fn metadata_parser_rejects_ambiguous_or_unknown_fields() {
        for metadata in [
            "KAEL_MDATA_version 0\nKAEL_MDATA_version 0\n",
            "KAEL_MDATA_version 0 trailing\n",
            "KAEL_MDATA_version 0\nKAEL_MDATA_unknown 1\n",
            "KAEL_MDATA_version 0\nKAEL_MDATA_weight 0\n",
        ] {
            assert!(matches!(
                parse_mdata_stdout(metadata),
                Err(FailKind::BadMetadata)
            ));
        }
    }

    #[test]
    fn compare_arguments_accept_the_documented_two_identifier_form() {
        let args = ["current".to_owned(), "baseline".to_owned()];
        let parsed = parse_compare_args(&args).unwrap();
        assert_eq!(parsed.new, "current");
        assert_eq!(parsed.old, "baseline");
        assert!(parsed.save_to.is_none());
    }

    #[test]
    fn compare_arguments_accept_an_explicit_report_path() {
        let args = [
            "--save=report.md".to_owned(),
            "current".to_owned(),
            "baseline".to_owned(),
        ];
        let parsed = parse_compare_args(&args).unwrap();
        assert_eq!(parsed.save_to, Some(Path::new("report.md")));
    }

    #[test]
    fn run_identifiers_cannot_escape_the_runs_directory() {
        for identifier in ["", "../outside", "with.dot", "with space", "/absolute"] {
            assert!(validate_run_identifier(identifier).is_err(), "{identifier}");
        }
        assert!(validate_run_identifier("feature_123-baseline").is_ok());
    }

    #[test]
    fn saved_run_matching_requires_an_identifier_boundary() {
        assert_eq!(run_file_component("main.kael.json", "main"), Some("kael"));
        assert_eq!(run_file_component("main-old.kael.json", "main"), None);
        assert_eq!(run_file_component("main.kael.json.bak", "main"), None);
    }

    #[test]
    fn generated_performance_tests_are_paired_by_exact_base_name() {
        let listing = format!(
            "ordinary::test: test\n\
             suite::render{}: test\n\
             suite::other{}: test\n\
             suite::render{}: test\n\
             suite::other{}: test\n",
            consts::SUF_MDATA,
            consts::SUF_NORMAL,
            consts::SUF_NORMAL,
            consts::SUF_MDATA,
        );

        assert_eq!(
            parse_test_list(&listing).unwrap(),
            [
                (
                    format!("suite::other{}", consts::SUF_NORMAL),
                    format!("suite::other{}", consts::SUF_MDATA),
                ),
                (
                    format!("suite::render{}", consts::SUF_NORMAL),
                    format!("suite::render{}", consts::SUF_MDATA),
                ),
            ]
        );
    }

    #[test]
    fn malformed_generated_performance_test_pairs_are_rejected() {
        let unpaired = format!("suite::render{}: test\n", consts::SUF_NORMAL);
        let duplicate = format!(
            "suite::render{}: test\nsuite::render{}: test\n",
            consts::SUF_NORMAL,
            consts::SUF_NORMAL,
        );
        let oversized = format!(
            "{}{}: test\n",
            "x".repeat(MAX_TEST_NAME_BYTES + 1),
            consts::SUF_NORMAL,
        );

        assert!(parse_test_list(&unpaired).is_err());
        assert!(parse_test_list(&duplicate).is_err());
        assert!(parse_test_list(&oversized).is_err());
    }

    #[test]
    fn missing_test_binaries_return_an_error_instead_of_panicking() {
        let missing = std::env::temp_dir().join(format!(
            "kael-perf-missing-test-binary-{}",
            std::process::id()
        ));

        assert!(get_tests(&missing.to_string_lossy()).is_err());
    }

    #[test]
    fn only_cargo_artifact_hashes_are_removed_from_binary_names() {
        assert_eq!(strip_test_binary_hash("kael-a1b2c3d4"), "kael");
        assert_eq!(strip_test_binary_hash("my-app"), "my-app");
    }

    #[test]
    fn hyperfine_json_is_parsed_by_field_name() {
        let timings = parse_hyperfine_timings(
            br#"{"results":[{"command":"test","mean":0.125,"stddev":0.005}]}"#,
        )
        .unwrap();
        assert_eq!(timings.mean, Duration::from_millis(125));
        assert_eq!(timings.stddev, Duration::from_millis(5));
    }

    #[test]
    fn hyperfine_json_rejects_invalid_or_ambiguous_results() {
        for json in [
            br#"{"results":[]}"#.as_slice(),
            br#"{"results":[{"mean":0.0,"stddev":0.0}]}"#.as_slice(),
            br#"{"results":[{"mean":1.0,"stddev":-1.0}]}"#.as_slice(),
            br#"{"results":[{"mean":1.0,"stddev":0.0},{"mean":2.0,"stddev":0.0}]}"#.as_slice(),
        ] {
            assert!(parse_hyperfine_timings(json).is_none());
        }
    }

    #[test]
    fn hyperfine_commands_quote_paths_without_enabling_a_shell() {
        assert_eq!(
            hyperfine_command("/tmp/Kael work/tests", "module::benchmark"),
            Some("\"/tmp/Kael work/tests\" --exact \"module::benchmark\"".to_owned())
        );
        assert!(hyperfine_command("/tmp/tests\ncommand", "benchmark").is_none());
    }
}
