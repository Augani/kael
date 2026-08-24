use serde_json::Value;
use std::ffi::OsStr;
use std::fs;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

const WASM_TARGET: &str = "wasm32-unknown-unknown";
const WASM_BINDGEN_VERSION: &str = "0.2.122";
const WASM_OPT_VERSION: &str = "132";

const DEFAULT_INDEX: &str = r##"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width,initial-scale=1" />
    <meta name="color-scheme" content="dark light" />
    <title>Kael application</title>
    <style>
      html, body, #blade { width: 100%; height: 100%; margin: 0; }
      body { overflow: hidden; background: #111827; }
      #blade { display: block; outline: none; touch-action: none; }
      #kael-error { position: fixed; inset: 1rem; color: #fecaca;
        font: 14px/1.5 ui-monospace, monospace; white-space: pre-wrap;
        pointer-events: none; }
    </style>
  </head>
  <body>
    <canvas id="blade" aria-label="Kael application"></canvas>
    <pre id="kael-error" role="alert"></pre>
    <script type="module">
      import init from "./app.js";
      try {
        await init({ module_or_path: "./app_bg.wasm" });
        document.documentElement.dataset.kaelReady = "true";
      } catch (error) {
        document.querySelector("#kael-error").textContent =
          `Kael browser startup failed:\n${error?.stack ?? error}`;
        throw error;
      }
    </script>
  </body>
</html>
"##;

#[derive(Debug, Clone, PartialEq, Eq)]
struct WebOptions {
    command: WebCommand,
    release: bool,
    out_dir: PathBuf,
    package: Option<String>,
    bin: Option<String>,
    port: u16,
    open: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WebCommand {
    Build,
    Serve,
}

struct BuildTarget {
    package: String,
    bin: String,
    target_directory: PathBuf,
}

pub(super) fn run(args: &[String]) -> Result<(), String> {
    if args.is_empty()
        || args
            .iter()
            .any(|argument| matches!(argument.as_str(), "-h" | "--help"))
    {
        print_help();
        return Ok(());
    }
    let options = parse_options(args)?;
    let current_dir = std::env::current_dir()
        .map_err(|error| format!("cannot resolve the current directory: {error}"))?;
    let output = build(&current_dir, &options)?;
    println!("Built Kael web app in {}", output.display());
    if options.command == WebCommand::Serve {
        serve(&output, options.port, options.open)?;
    } else {
        println!("Run it locally with: kael web serve");
    }
    Ok(())
}

fn print_help() {
    println!(
        "Build the same Kael app for a WebGL2 browser canvas.

USAGE:
    kael web build [OPTIONS]
    kael web serve [OPTIONS]

OPTIONS:
    --debug              Build without release optimizations
    --out-dir <path>     Output directory (default: dist/web)
    --package <name>     Select a package in a Cargo workspace
    --bin <name>         Select a binary target
    --port <number>      Local serve port (default: 8000)
    --no-open            Do not open the browser after serving
    -h, --help           Print this help"
    );
}

fn parse_options(args: &[String]) -> Result<WebOptions, String> {
    let command = match args.first().map(String::as_str) {
        Some("build") => WebCommand::Build,
        Some("serve") => WebCommand::Serve,
        Some(other) => return Err(format!("unknown `kael web` command {other:?}")),
        None => return Err("`kael web` requires `build` or `serve`".into()),
    };
    let mut options = WebOptions {
        command,
        release: true,
        out_dir: PathBuf::from("dist/web"),
        package: None,
        bin: None,
        port: 8_000,
        open: true,
    };
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--debug" => options.release = false,
            "--no-open" => options.open = false,
            "--out-dir" => {
                index += 1;
                options.out_dir = required_value(args, index, "--out-dir")?.into();
            }
            "--package" => {
                index += 1;
                options.package = Some(required_value(args, index, "--package")?.into());
            }
            "--bin" => {
                index += 1;
                options.bin = Some(required_value(args, index, "--bin")?.into());
            }
            "--port" => {
                index += 1;
                options.port = required_value(args, index, "--port")?
                    .parse()
                    .map_err(|_| "--port must be an integer from 1 to 65535".to_string())?;
                if options.port == 0 {
                    return Err("--port must be an integer from 1 to 65535".into());
                }
            }
            other => return Err(format!("unknown web option {other:?}")),
        }
        index += 1;
    }
    if command == WebCommand::Build && (!options.open || options.port != 8_000) {
        return Err("--port and --no-open are only valid with `kael web serve`".into());
    }
    Ok(options)
}

fn required_value<'a>(args: &'a [String], index: usize, option: &str) -> Result<&'a str, String> {
    args.get(index)
        .map(String::as_str)
        .filter(|value| !value.starts_with('-'))
        .ok_or_else(|| format!("{option} requires a value"))
}

fn build(current_dir: &Path, options: &WebOptions) -> Result<PathBuf, String> {
    let target = discover_target(
        current_dir,
        options.package.as_deref(),
        options.bin.as_deref(),
    )?;
    ensure_wasm_bindgen()?;
    if options.release {
        ensure_wasm_opt()?;
    }

    let mut cargo = Command::new("cargo");
    cargo.current_dir(current_dir).args([
        "build",
        "--target",
        WASM_TARGET,
        "--package",
        &target.package,
        "--bin",
        &target.bin,
    ]);
    if options.release {
        cargo.arg("--release");
    }
    let status = cargo
        .status()
        .map_err(|error| format!("failed to start Cargo: {error}"))?;
    if !status.success() {
        return Err(format!(
            "Cargo could not build {} for {WASM_TARGET}; ensure Kael dependencies select their `browser` features for wasm32",
            target.bin
        ));
    }

    let profile = if options.release { "release" } else { "debug" };
    let wasm_dir = target.target_directory.join(WASM_TARGET).join(profile);
    let wasm = find_wasm(&wasm_dir, &target.bin)?;
    let output = if options.out_dir.is_absolute() {
        options.out_dir.clone()
    } else {
        current_dir.join(&options.out_dir)
    };
    fs::create_dir_all(&output)
        .map_err(|error| format!("failed to create {}: {error}", output.display()))?;

    let status = Command::new("wasm-bindgen")
        .args([
            "--target",
            "web",
            "--no-typescript",
            "--out-name",
            "app",
            "--out-dir",
        ])
        .arg(&output)
        .arg(&wasm)
        .status()
        .map_err(|error| format!("failed to run wasm-bindgen: {error}"))?;
    if !status.success() {
        return Err("wasm-bindgen could not package the compiled module".into());
    }
    if options.release {
        optimize_wasm(&output.join("app_bg.wasm"))?;
    }

    let index = output.join("index.html");
    if !index.exists() {
        fs::write(&index, DEFAULT_INDEX)
            .map_err(|error| format!("failed to write {}: {error}", index.display()))?;
    }
    Ok(output)
}

fn discover_target(
    current_dir: &Path,
    requested_package: Option<&str>,
    requested_bin: Option<&str>,
) -> Result<BuildTarget, String> {
    let output = Command::new("cargo")
        .current_dir(current_dir)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .map_err(|error| format!("failed to run `cargo metadata`: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "`cargo metadata` failed:\n{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let metadata: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("Cargo returned invalid metadata: {error}"))?;
    select_target(&metadata, current_dir, requested_package, requested_bin)
}

fn select_target(
    metadata: &Value,
    current_dir: &Path,
    requested_package: Option<&str>,
    requested_bin: Option<&str>,
) -> Result<BuildTarget, String> {
    let packages = metadata["packages"]
        .as_array()
        .ok_or_else(|| "Cargo metadata did not contain packages".to_string())?;
    let package = if let Some(name) = requested_package {
        packages
            .iter()
            .find(|package| package["name"].as_str() == Some(name))
            .ok_or_else(|| format!("Cargo workspace has no package named {name:?}"))?
    } else {
        let local_manifest = current_dir.join("Cargo.toml");
        let local = packages.iter().find(|package| {
            package["manifest_path"]
                .as_str()
                .is_some_and(|path| paths_equivalent(Path::new(path), &local_manifest))
        });
        if let Some(package) = local {
            package
        } else {
            let defaults = metadata["workspace_default_members"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>();
            if defaults.len() != 1 {
                return Err("select a Cargo package with `--package <name>`".into());
            }
            packages
                .iter()
                .find(|package| package["id"].as_str() == Some(defaults[0]))
                .ok_or_else(|| "Cargo default package was not present in metadata".to_string())?
        }
    };
    let package_name = package["name"]
        .as_str()
        .ok_or_else(|| "Cargo package had no name".to_string())?;
    let bins = package["targets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|target| {
            target["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind.as_str() == Some("bin")))
        })
        .collect::<Vec<_>>();
    let bin = if let Some(name) = requested_bin {
        bins.iter()
            .copied()
            .find(|target| target["name"].as_str() == Some(name))
            .ok_or_else(|| format!("package {package_name:?} has no binary named {name:?}"))?
    } else if bins.len() == 1 {
        bins[0]
    } else if let Some(target) = bins
        .iter()
        .copied()
        .find(|target| target["name"].as_str() == Some(package_name))
    {
        target
    } else {
        return Err("select a binary target with `--bin <name>`".into());
    };
    let bin = bin["name"]
        .as_str()
        .ok_or_else(|| "Cargo binary target had no name".to_string())?;
    let target_directory = metadata["target_directory"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| "Cargo metadata had no target directory".to_string())?;
    Ok(BuildTarget {
        package: package_name.into(),
        bin: bin.into(),
        target_directory,
    })
}

fn paths_equivalent(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn find_wasm(directory: &Path, bin: &str) -> Result<PathBuf, String> {
    for file_name in [
        format!("{bin}.wasm"),
        format!("{}.wasm", bin.replace('-', "_")),
    ] {
        let candidate = directory.join(file_name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(format!(
        "Cargo succeeded but {} did not contain the {bin:?} WebAssembly binary",
        directory.display()
    ))
}

fn ensure_wasm_bindgen() -> Result<(), String> {
    let output = Command::new("wasm-bindgen")
        .arg("--version")
        .output()
        .map_err(|_| install_wasm_bindgen_message())?;
    let version = String::from_utf8_lossy(&output.stdout);
    if !output.status.success()
        || !version
            .split_whitespace()
            .any(|part| part == WASM_BINDGEN_VERSION)
    {
        return Err(format!(
            "Kael requires wasm-bindgen CLI {WASM_BINDGEN_VERSION}, but found {:?}. {}",
            version.trim(),
            install_wasm_bindgen_message()
        ));
    }
    Ok(())
}

fn install_wasm_bindgen_message() -> String {
    format!(
        "Install it with `cargo install wasm-bindgen-cli --version {WASM_BINDGEN_VERSION} --locked`."
    )
}

fn ensure_wasm_opt() -> Result<(), String> {
    let output = Command::new("wasm-opt")
        .arg("--version")
        .output()
        .map_err(|_| install_wasm_opt_message())?;
    let version = String::from_utf8_lossy(&output.stdout);
    if !output.status.success()
        || !version
            .split_whitespace()
            .any(|part| part == WASM_OPT_VERSION)
    {
        return Err(format!(
            "Kael requires wasm-opt {WASM_OPT_VERSION} for release builds, but found {:?}. {}",
            version.trim(),
            install_wasm_opt_message()
        ));
    }
    Ok(())
}

fn install_wasm_opt_message() -> String {
    "Install it with `npm install --global binaryen@132.0.0`. Use `--debug` only when an unoptimized development build is intentional.".into()
}

fn optimize_wasm(wasm: &Path) -> Result<(), String> {
    let optimized = wasm.with_extension("optimized.wasm");
    let before = fs::metadata(wasm)
        .map_err(|error| format!("cannot inspect {}: {error}", wasm.display()))?
        .len();
    let status = Command::new("wasm-opt")
        .arg("-O3")
        .arg(wasm)
        .arg("-o")
        .arg(&optimized)
        .status()
        .map_err(|error| format!("failed to start wasm-opt: {error}"))?;
    if !status.success() {
        let _ = fs::remove_file(&optimized);
        return Err("wasm-opt could not optimize the packaged module".into());
    }
    fs::copy(&optimized, wasm).map_err(|error| {
        format!(
            "failed to install optimized WebAssembly at {}: {error}",
            wasm.display()
        )
    })?;
    fs::remove_file(&optimized).map_err(|error| {
        format!(
            "failed to remove temporary optimized module {}: {error}",
            optimized.display()
        )
    })?;
    let after = fs::metadata(wasm)
        .map_err(|error| format!("cannot inspect {}: {error}", wasm.display()))?
        .len();
    println!("Optimized WebAssembly with Binaryen {WASM_OPT_VERSION}: {before} -> {after} bytes");
    Ok(())
}

fn serve(directory: &Path, port: u16, open: bool) -> Result<(), String> {
    let root = directory
        .canonicalize()
        .map_err(|error| format!("failed to resolve {}: {error}", directory.display()))?;
    let address = format!("127.0.0.1:{port}");
    let listener = TcpListener::bind(&address)
        .map_err(|error| format!("cannot serve http://{address}: {error}"))?;
    let url = format!("http://{address}");
    println!("Serving {} at {url} (press Ctrl-C to stop)", root.display());
    if open {
        open_browser(&url);
    }
    for connection in listener.incoming() {
        match connection {
            Ok(stream) => {
                if let Err(error) = serve_connection(stream, &root) {
                    eprintln!("web request failed: {error}");
                }
            }
            Err(error) => eprintln!("web connection failed: {error}"),
        }
    }
    Ok(())
}

fn serve_connection(mut stream: TcpStream, root: &Path) -> Result<(), String> {
    let mut reader = BufReader::new(
        stream
            .try_clone()
            .map_err(|error| format!("failed to read request: {error}"))?,
    );
    let mut request = String::new();
    reader
        .read_line(&mut request)
        .map_err(|error| format!("failed to read request: {error}"))?;
    let mut fields = request.split_whitespace();
    let method = fields.next().unwrap_or_default();
    let uri = fields.next().unwrap_or_default();
    let head = method == "HEAD";
    if method != "GET" && !head {
        return write_response(
            &mut stream,
            405,
            "text/plain; charset=utf-8",
            b"Method Not Allowed",
            head,
        );
    }
    let path = match safe_asset_path(root, uri) {
        Some(path) => path,
        None => {
            return write_response(
                &mut stream,
                404,
                "text/plain; charset=utf-8",
                b"Not Found",
                head,
            );
        }
    };
    let mut body = Vec::new();
    fs::File::open(&path)
        .and_then(|mut file| file.read_to_end(&mut body))
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    write_response(&mut stream, 200, mime_type(&path), &body, head)
}

fn safe_asset_path(root: &Path, uri: &str) -> Option<PathBuf> {
    let raw = uri.split(['?', '#']).next()?;
    if raw.contains('%') || raw.contains('\\') {
        return None;
    }
    let relative = raw.trim_start_matches('/');
    let relative = if relative.is_empty() {
        "index.html"
    } else {
        relative
    };
    let path = Path::new(relative);
    if !path
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
    {
        return None;
    }
    let candidate = root.join(path).canonicalize().ok()?;
    (candidate.starts_with(root) && candidate.is_file()).then_some(candidate)
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
    head: bool,
) -> Result<(), String> {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .and_then(|()| if head { Ok(()) } else { stream.write_all(body) })
    .map_err(|error| format!("failed to write response: {error}"))
}

fn mime_type(path: &Path) -> &'static str {
    match path.extension().and_then(OsStr::to_str) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(url);
        command
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };
    #[cfg(not(any(unix, target_os = "windows")))]
    return;

    command.stdout(Stdio::null()).stderr(Stdio::null());
    if let Err(error) = command.spawn() {
        eprintln!("could not open the browser automatically ({error}); open {url}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_build_and_serve_options() {
        let build = parse_options(&[
            "build".into(),
            "--debug".into(),
            "--bin".into(),
            "demo".into(),
        ])
        .unwrap();
        assert_eq!(build.command, WebCommand::Build);
        assert!(!build.release);
        assert_eq!(build.bin.as_deref(), Some("demo"));

        let serve = parse_options(&[
            "serve".into(),
            "--port".into(),
            "9000".into(),
            "--no-open".into(),
        ])
        .unwrap();
        assert_eq!(serve.command, WebCommand::Serve);
        assert_eq!(serve.port, 9_000);
        assert!(!serve.open);
    }

    #[test]
    fn build_rejects_serve_only_options() {
        assert!(parse_options(&["build".into(), "--port".into(), "9000".into()]).is_err());
        assert!(parse_options(&["build".into(), "--no-open".into()]).is_err());
    }

    #[test]
    fn selects_local_package_and_default_binary() {
        let temp = std::env::temp_dir().join(format!("kael-web-metadata-{}", std::process::id()));
        fs::create_dir_all(&temp).unwrap();
        let manifest = temp.join("Cargo.toml");
        fs::write(&manifest, "[package]\nname='demo'\nversion='0.1.0'\n").unwrap();
        let metadata = serde_json::json!({
            "target_directory": temp.join("target"),
            "workspace_default_members": ["demo 0.1.0"],
            "packages": [{
                "id": "demo 0.1.0",
                "name": "demo",
                "manifest_path": manifest,
                "targets": [{"name": "demo", "kind": ["bin"]}]
            }]
        });
        let target = select_target(&metadata, &temp, None, None).unwrap();
        assert_eq!(target.package, "demo");
        assert_eq!(target.bin, "demo");
        fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn static_asset_paths_cannot_escape_the_output_root() {
        let temp = std::env::temp_dir().join(format!("kael-web-assets-{}", std::process::id()));
        fs::create_dir_all(&temp).unwrap();
        fs::write(temp.join("index.html"), DEFAULT_INDEX).unwrap();
        let root = temp.canonicalize().unwrap();
        assert_eq!(safe_asset_path(&root, "/"), Some(root.join("index.html")));
        assert!(safe_asset_path(&root, "/../Cargo.toml").is_none());
        assert!(safe_asset_path(&root, "/%2e%2e/Cargo.toml").is_none());
        fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn default_page_has_the_required_canvas_and_module() {
        assert!(DEFAULT_INDEX.contains("<canvas id=\"blade\""));
        assert!(DEFAULT_INDEX.contains("./app.js"));
        assert!(DEFAULT_INDEX.contains("./app_bg.wasm"));
        assert!(DEFAULT_INDEX.contains("module_or_path"));
    }
}
