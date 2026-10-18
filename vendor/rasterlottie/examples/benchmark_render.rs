//! Benchmark helper for comparing rasterlottie render throughput.
#![allow(clippy::print_stdout, clippy::print_stderr, reason = "this is example")]

use std::{
    env, fs,
    path::{Path, PathBuf},
    process,
    time::Instant,
};

use rasterlottie::{Animation, Pixmap, RenderConfig, Renderer, Rgba8};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BenchmarkMode {
    Full,
    Gif,
}

#[derive(Debug)]
struct BenchmarkOptions {
    input: PathBuf,
    mode: BenchmarkMode,
    max_fps: f32,
    scale: f32,
    background: Rgba8,
    json: bool,
}

#[derive(Debug, Clone)]
struct BenchmarkSchedule {
    mode: BenchmarkMode,
    source_frames: Vec<f32>,
    source_fps: f32,
    requested_fps: f32,
    actual_output_fps: f32,
    start_frame: f32,
    end_frame: f32,
}

#[derive(Debug, Serialize)]
struct BenchmarkResult {
    renderer: &'static str,
    mode: &'static str,
    source_fps: f32,
    requested_fps: f32,
    actual_output_fps: f32,
    start_frame: f32,
    end_frame: f32,
    rendered_frames: usize,
    output_width: u32,
    output_height: u32,
    scale: f32,
    background_rgba: [u8; 4],
    elapsed_ms: f64,
    avg_ms_per_frame: f64,
}

impl BenchmarkMode {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "full" => Ok(Self::Full),
            "gif" => Ok(Self::Gif),
            other => Err(format!(
                "unsupported benchmark mode `{other}`. expected `full` or `gif`"
            )),
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Gif => "gif",
        }
    }
}

impl BenchmarkOptions {
    fn parse<I>(mut args: I) -> Result<Self, String>
    where
        I: Iterator<Item = String>,
    {
        let mut input = None;
        let mut mode = BenchmarkMode::Full;
        let mut max_fps = 60.0;
        let mut scale = 1.0;
        let mut background = Rgba8::TRANSPARENT;
        let mut json = false;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--mode" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value after `--mode`".to_string())?;
                    mode = BenchmarkMode::parse(&value)?;
                }
                "--max-fps" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value after `--max-fps`".to_string())?;
                    max_fps = value.parse::<f32>().map_err(|error| {
                        format!("failed to parse `--max-fps` value `{value}`: {error}")
                    })?;
                }
                "--scale" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value after `--scale`".to_string())?;
                    scale = value.parse::<f32>().map_err(|error| {
                        format!("failed to parse `--scale` value `{value}`: {error}")
                    })?;
                }
                "--background" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value after `--background`".to_string())?;
                    background = parse_rgba8(&value)?;
                }
                "--json" => {
                    json = true;
                }
                "--help" | "-h" => {
                    print_usage();
                    process::exit(0);
                }
                value if value.starts_with("--") => {
                    return Err(format!("unknown flag `{value}`"));
                }
                value => {
                    if input.is_some() {
                        return Err(format!(
                            "unexpected extra positional argument `{value}`. only one input animation path is supported"
                        ));
                    }
                    input = Some(PathBuf::from(value));
                }
            }
        }

        let input = input.ok_or_else(|| {
            "missing input animation path. run with `--help` for usage information".to_string()
        })?;
        if !max_fps.is_finite() || max_fps <= 0.0 {
            return Err(format!(
                "`--max-fps` must be a positive finite number, got {max_fps}"
            ));
        }
        if !scale.is_finite() || scale <= 0.0 {
            return Err(format!(
                "`--scale` must be a positive finite number, got {scale}"
            ));
        }

        Ok(Self {
            input,
            mode,
            max_fps,
            scale,
            background,
            json,
        })
    }
}

impl BenchmarkSchedule {
    fn resolve(animation: &Animation, mode: BenchmarkMode, max_fps: f32) -> Self {
        let source_fps = animation.frame_rate.max(1.0);
        let start_frame = animation.in_point.floor();
        let end_frame = animation.out_point.ceil().max(start_frame + 1.0);
        match mode {
            BenchmarkMode::Full => {
                let source_frames = (0..((end_frame - start_frame).max(0.0) as usize))
                    .map(|index| start_frame + index as f32)
                    .collect();
                Self {
                    mode,
                    source_frames,
                    source_fps,
                    requested_fps: source_fps,
                    actual_output_fps: source_fps,
                    start_frame,
                    end_frame,
                }
            }
            BenchmarkMode::Gif => {
                let requested_fps = source_fps.min(max_fps.max(1.0));
                let frame_delay = ((100.0 / requested_fps).round() as u16).max(1);
                let actual_output_fps = 100.0 / frame_delay as f32;
                let max_output_frames = (actual_output_fps * animation.duration_seconds().max(0.1))
                    .floor()
                    .max(1.0) as usize;
                let source_frame_step = source_fps / actual_output_fps;
                let source_frames = (0..max_output_frames)
                    .map(|index| (index as f32).mul_add(source_frame_step, start_frame))
                    .take_while(|frame| *frame < end_frame)
                    .collect();
                Self {
                    mode,
                    source_frames,
                    source_fps,
                    requested_fps,
                    actual_output_fps,
                    start_frame,
                    end_frame,
                }
            }
        }
    }
}

impl BenchmarkResult {
    fn from_run(
        renderer: &'static str,
        schedule: &BenchmarkSchedule,
        config: RenderConfig,
        pixmap: &Pixmap,
        elapsed_ms: f64,
    ) -> Self {
        let rendered_frames = schedule.source_frames.len().max(1);
        Self {
            renderer,
            mode: schedule.mode.as_str(),
            source_fps: schedule.source_fps,
            requested_fps: schedule.requested_fps,
            actual_output_fps: schedule.actual_output_fps,
            start_frame: schedule.start_frame,
            end_frame: schedule.end_frame,
            rendered_frames,
            output_width: pixmap.width(),
            output_height: pixmap.height(),
            scale: config.scale,
            background_rgba: [
                config.background.r,
                config.background.g,
                config.background.b,
                config.background.a,
            ],
            elapsed_ms,
            avg_ms_per_frame: elapsed_ms / rendered_frames as f64,
        }
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let options = BenchmarkOptions::parse(env::args().skip(1))?;
    let animation = load_animation(&options.input)?;
    let prepared = Renderer::default()
        .prepare(&animation)
        .map_err(|error| format!("failed to prepare animation: {error}"))?;
    let config = RenderConfig::new(options.background, options.scale);
    let schedule = BenchmarkSchedule::resolve(prepared.animation(), options.mode, options.max_fps);
    let mut pixmap = prepared
        .new_scratch_pixmap_for_config(config)
        .map_err(|error| format!("failed to allocate scratch pixmap: {error}"))?;

    let start = Instant::now();
    for source_frame in &schedule.source_frames {
        prepared
            .render_frame_into_pixmap(*source_frame, config, &mut pixmap)
            .map_err(|error| format!("failed to render frame {source_frame}: {error}"))?;
    }
    let elapsed = start.elapsed();

    let result = BenchmarkResult::from_run(
        "rasterlottie",
        &schedule,
        config,
        &pixmap,
        elapsed.as_secs_f64() * 1000.0,
    );
    if options.json {
        println!(
            "{}",
            serde_json::to_string(&result)
                .map_err(|error| format!("failed to serialize benchmark result: {error}"))?
        );
    } else {
        print_human_readable(&result);
    }

    Ok(())
}

fn print_human_readable(result: &BenchmarkResult) {
    println!("renderer: {}", result.renderer);
    println!("mode: {}", result.mode);
    println!("source_fps: {:.3}", result.source_fps);
    println!("requested_fps: {:.3}", result.requested_fps);
    println!("actual_output_fps: {:.3}", result.actual_output_fps);
    println!("start_frame: {:.3}", result.start_frame);
    println!("end_frame: {:.3}", result.end_frame);
    println!("rendered_frames: {}", result.rendered_frames);
    println!(
        "output_size: {}x{}",
        result.output_width, result.output_height
    );
    println!("scale: {:.3}", result.scale);
    println!(
        "background_rgba: #{:02X}{:02X}{:02X}{:02X}",
        result.background_rgba[0],
        result.background_rgba[1],
        result.background_rgba[2],
        result.background_rgba[3]
    );
    println!("elapsed_ms: {:.3}", result.elapsed_ms);
    println!("avg_ms_per_frame: {:.3}", result.avg_ms_per_frame);
}

fn parse_rgba8(value: &str) -> Result<Rgba8, String> {
    let normalized = value.trim().trim_start_matches('#');
    match normalized.len() {
        6 => {
            let rgb = u32::from_str_radix(normalized, 16)
                .map_err(|error| format!("invalid background color `{value}`: {error}"))?;
            Ok(Rgba8::new(
                ((rgb >> 16) & 0xff) as u8,
                ((rgb >> 8) & 0xff) as u8,
                (rgb & 0xff) as u8,
                0xff,
            ))
        }
        8 => {
            let rgba = u32::from_str_radix(normalized, 16)
                .map_err(|error| format!("invalid background color `{value}`: {error}"))?;
            Ok(Rgba8::new(
                ((rgba >> 24) & 0xff) as u8,
                ((rgba >> 16) & 0xff) as u8,
                ((rgba >> 8) & 0xff) as u8,
                (rgba & 0xff) as u8,
            ))
        }
        _ => Err(format!(
            "invalid background color `{value}`. expected RRGGBB or RRGGBBAA"
        )),
    }
}

fn print_usage() {
    println!("Usage:");
    println!(
        "  cargo run --example benchmark_render -- <input.json|input.lottie> [--mode full|gif] [--max-fps <float>] [--scale <float>] [--background <hex>] [--json]"
    );
    println!();
    println!("Modes:");
    println!("  full  Render every source frame between in_point and out_point.");
    println!("  gif   Render the sampled frame sequence used by render-gif timing.");
}

fn load_animation(path: &Path) -> Result<Animation, String> {
    if path_uses_dotlottie(path) {
        #[cfg(feature = "dotlottie")]
        {
            let bytes = fs::read(path)
                .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
            return Animation::from_dotlottie_bytes(&bytes)
                .map_err(|error| format!("failed to parse {}: {error}", path.display()));
        }
        #[cfg(not(feature = "dotlottie"))]
        {
            return Err(
                "`.lottie` input requires building benchmark_render with `--features dotlottie`"
                    .to_string(),
            );
        }
    }

    let json = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    Animation::from_json_str(&json)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))
}

fn path_uses_dotlottie(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("lottie"))
}
