//! Command-line utilities for analyzing and rendering Lottie files.
#![allow(clippy::print_stdout, clippy::print_stderr, reason = "this is example")]

use std::{
    collections::BTreeMap,
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command as ProcessCommand, Output, Stdio, exit},
    str,
};

use image::RgbaImage;
#[cfg(feature = "gif")]
use rasterlottie::GifRenderConfig;
use rasterlottie::{
    Animation, Asset, Layer, RenderConfig, Renderer, Rgba8, ShapeItem, analyze_animation,
};
use seahorse::{
    ActionError, ActionResult, App, Command, Context, Flag, FlagType, error::FlagError,
};

struct Mp4EncodeOptions {
    codec: Mp4Codec,
    pixel_format: String,
    quality: Mp4QualityMode,
    preset: String,
}

#[derive(Clone, Copy)]
enum Mp4Codec {
    Libx264,
    Libx264Rgb,
}

enum Mp4QualityMode {
    Crf(u8),
    Lossless,
}

struct RenderTiming {
    target_fps: f32,
    output_frames: usize,
    output_duration_seconds: f32,
}

#[derive(Default)]
struct AnimationStats {
    top_level_layers: usize,
    asset_layer_definitions: usize,
    total_layer_definitions: usize,
    hidden_layers: usize,
    parented_layers: usize,
    masked_layers: usize,
    masks_total: usize,
    track_matte_layers: usize,
    matte_source_layers: usize,
    effects_layers: usize,
    effects_total: usize,
    time_remap_layers: usize,
    text_layers_with_animators: usize,
    text_layers_with_path: usize,
    shape_items_total: usize,
    assets_total: usize,
    precomp_assets: usize,
    image_assets: usize,
    embedded_image_assets: usize,
    external_image_assets: usize,
    layer_type_counts: BTreeMap<String, usize>,
    shape_item_type_counts: BTreeMap<String, usize>,
}

impl Mp4Codec {
    fn parse(value: &str) -> Result<Self, ActionError> {
        match value.trim() {
            "libx264" => Ok(Self::Libx264),
            "libx264rgb" => Ok(Self::Libx264Rgb),
            other => Err(to_action_error(&format!(
                "invalid codec `{other}`: expected libx264 or libx264rgb"
            ))),
        }
    }

    const fn ffmpeg_name(self) -> &'static str {
        match self {
            Self::Libx264 => "libx264",
            Self::Libx264Rgb => "libx264rgb",
        }
    }

    const fn default_pixel_format(self) -> &'static str {
        match self {
            Self::Libx264 => "yuv420p",
            Self::Libx264Rgb => "rgb24",
        }
    }

    fn validate_pixel_format(self, value: &str) -> Result<String, ActionError> {
        let value = value.trim();
        let allowed = match self {
            Self::Libx264 => &["yuv420p", "yuv444p"][..],
            Self::Libx264Rgb => &["rgb24", "bgr24", "bgr0"][..],
        };
        if allowed.contains(&value) {
            Ok(value.to_string())
        } else {
            Err(to_action_error(&format!(
                "invalid pixel format `{value}` for {}: expected one of {}",
                self.ffmpeg_name(),
                allowed.join(", ")
            )))
        }
    }
}

impl Mp4EncodeOptions {
    fn describe_quality_mode(&self) -> String {
        match self.quality {
            Mp4QualityMode::Crf(value) => format!("crf={value}"),
            Mp4QualityMode::Lossless => "lossless".to_string(),
        }
    }

    fn ffmpeg_quality_args(&self) -> Vec<String> {
        match self.quality {
            Mp4QualityMode::Crf(value) => vec!["-crf".to_string(), value.to_string()],
            Mp4QualityMode::Lossless => vec!["-qp".to_string(), "0".to_string()],
        }
    }
}

fn main() {
    #[cfg(feature = "tracing")]
    if let Err(error) = init_tracing_if_requested() {
        eprintln!("{error}");
        exit(1);
    }
    #[cfg(not(feature = "tracing"))]
    init_tracing_if_requested();

    let app = App::new("rasterlottie-cli")
        .description("Analyze or render Lottie JSON or .lottie archives with rasterlottie.")
        .command(
            Command::new("analyze")
                .description("Load a Lottie JSON or .lottie file and print the support report.")
                .usage("cargo run --example rasterlottie_cli -- analyze <input.json|input.lottie>")
                .action_with_result(analyze_command),
        )
        .command(
            Command::new("render-png")
                .description("Render a single Lottie JSON or .lottie frame into a PNG.")
                .usage(
                    "cargo run --example rasterlottie_cli -- render-png <input.json|input.lottie> <output.png> [--frame <float>] [--background <hex>] [--scale <float>]",
                )
                .flag(
                    Flag::new("frame", FlagType::Float)
                        .alias("f")
                        .description("Animation frame to render."),
                )
                .flag(
                    Flag::new("background", FlagType::String)
                        .alias("b")
                        .description("Optional background color in RRGGBB or RRGGBBAA form."),
                )
                .flag(
                    Flag::new("scale", FlagType::Float)
                        .alias("s")
                        .description("Optional supersampling scale factor for the output size."),
                )
                .action_with_result(render_png_command),
        )
        .command(
            Command::new("render-mp4")
                .description("Render a Lottie JSON or .lottie file into an MP4 with ffmpeg.")
                .usage(
                    "cargo run --example rasterlottie_cli -- render-mp4 <input.json|input.lottie> <output.mp4> [--fps <float>] [--duration <float>] [--background <hex>] [--scale <float>] [--crf <int>] [--preset <name>] [--codec <name>] [--pix-fmt <name>] [--lossless]",
                )
                .flag(
                    Flag::new("fps", FlagType::Float)
                        .alias("f")
                        .description("Maximum output MP4 frame rate."),
                )
                .flag(
                    Flag::new("duration", FlagType::Float)
                        .alias("d")
                        .description("Maximum output MP4 duration in seconds."),
                )
                .flag(
                    Flag::new("background", FlagType::String)
                        .alias("b")
                        .description("Optional background color in RRGGBB or RRGGBBAA form."),
                )
                .flag(
                    Flag::new("scale", FlagType::Float)
                        .alias("s")
                        .description("Optional supersampling scale factor for the output size."),
                )
                .flag(
                    Flag::new("crf", FlagType::Int)
                        .description("Optional libx264 CRF quality value, where lower is higher quality."),
                )
                .flag(
                    Flag::new("preset", FlagType::String)
                        .description("Optional libx264 preset such as ultrafast, medium, slow, or veryslow."),
                )
                .flag(
                    Flag::new("codec", FlagType::String)
                        .description("Optional MP4 video codec, currently libx264 or libx264rgb."),
                )
                .flag(
                    Flag::new("pix-fmt", FlagType::String)
                        .description("Optional output pixel format, such as yuv420p, yuv444p, rgb24, bgr24, or bgr0."),
                )
                .flag(
                    Flag::new("lossless", FlagType::Bool)
                        .description("Use codec lossless mode. For RGB-preserving output, combine with --codec libx264rgb."),
                )
                .action_with_result(render_mp4_command),
        );
    #[cfg(feature = "gif")]
    let app = app.command(render_gif_subcommand());

    if let Err(error) = app.run_with_result(env::args().collect()) {
        eprintln!("{}", error.message);
        exit(1);
    }
}

#[cfg(feature = "gif")]
fn render_gif_subcommand() -> Command {
    Command::new("render-gif")
        .description("Render a Lottie JSON or .lottie file into a GIF.")
        .usage(
            "cargo run --example rasterlottie_cli -- render-gif <input.json|input.lottie> <output.gif> [--fps <float>] [--duration <float>] [--quantizer-speed <int>]",
        )
        .flag(
            Flag::new("fps", FlagType::Float)
                .alias("f")
                .description("Maximum output GIF frame rate."),
        )
        .flag(
            Flag::new("duration", FlagType::Float)
                .alias("d")
                .description("Maximum output GIF duration in seconds."),
        )
        .flag(
            Flag::new("quantizer-speed", FlagType::Int)
                .alias("q")
                .description(
                    "GIF palette quantizer speed in the range 1..30. Higher is faster and lower quality.",
                ),
        )
        .action_with_result(render_gif_command)
}

#[cfg(feature = "tracing")]
fn init_tracing_if_requested() -> Result<(), String> {
    use tracing_subscriber::{EnvFilter, fmt::format::FmtSpan};

    if env::var_os("RASTERLOTTIE_TRACE").is_none() {
        return Ok(());
    }

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("rasterlottie=trace,rasterlottie_cli=debug"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_span_events(FmtSpan::CLOSE)
        .with_target(false)
        .compact()
        .try_init()
        .map_err(|error| format!("failed to initialize tracing subscriber: {error}"))
}

#[cfg(not(feature = "tracing"))]
fn init_tracing_if_requested() {
    if env::var_os("RASTERLOTTIE_TRACE").is_some() {
        eprintln!(
            "RASTERLOTTIE_TRACE is ignored because rasterlottie-cli was built without `--features tracing`."
        );
    }
}

fn analyze_command(context: &Context) -> ActionResult {
    let input = required_path_arg(context, 0, "input animation path")?;
    let animation = load_animation(&input)?;
    let report = analyze_animation(&animation);
    let stats = collect_animation_stats(&animation);

    println!("input: {}", input.display());
    println!("version: {}", animation.version);
    println!("canvas: {}x{}", animation.width, animation.height);
    println!("frame_rate: {:.3}", animation.frame_rate);
    println!("in_point: {:.3}", animation.in_point);
    println!("out_point: {:.3}", animation.out_point);
    println!("duration_frames: {:.3}", animation.duration_frames());
    println!("duration_seconds: {:.3}", animation.duration_seconds());
    println!("top_level_layers: {}", stats.top_level_layers);
    println!("asset_layer_definitions: {}", stats.asset_layer_definitions);
    println!("total_layer_definitions: {}", stats.total_layer_definitions);
    println!("layer_types: {}", format_counts(&stats.layer_type_counts));
    println!("hidden_layers: {}", stats.hidden_layers);
    println!("parented_layers: {}", stats.parented_layers);
    println!("masked_layers: {}", stats.masked_layers);
    println!("masks_total: {}", stats.masks_total);
    println!("track_matte_layers: {}", stats.track_matte_layers);
    println!("matte_source_layers: {}", stats.matte_source_layers);
    println!("effects_layers: {}", stats.effects_layers);
    println!("effects_total: {}", stats.effects_total);
    println!("time_remap_layers: {}", stats.time_remap_layers);
    println!(
        "text_layers_with_animators: {}",
        stats.text_layers_with_animators
    );
    println!("text_layers_with_path: {}", stats.text_layers_with_path);
    println!("shape_items_total: {}", stats.shape_items_total);
    println!(
        "shape_item_types: {}",
        format_counts(&stats.shape_item_type_counts)
    );
    println!("assets_total: {}", stats.assets_total);
    println!("precomp_assets: {}", stats.precomp_assets);
    println!("image_assets: {}", stats.image_assets);
    println!("embedded_image_assets: {}", stats.embedded_image_assets);
    println!("external_image_assets: {}", stats.external_image_assets);
    println!("fonts: {}", animation.fonts.list.len());
    println!("glyphs: {}", animation.chars.len());
    println!("supported: {}", report.is_supported());
    println!("issue_count: {}", report.len());
    if report.is_supported() {
        println!("No unsupported features detected.");
    } else {
        for issue in &report.issues {
            println!("- {:?} {}: {}", issue.kind, issue.path, issue.detail);
        }
    }
    Ok(())
}

#[cfg(feature = "gif")]
fn render_gif_command(context: &Context) -> ActionResult {
    #[cfg(feature = "tracing")]
    let _render_gif_command_span = tracing::debug_span!("render_gif_command").entered();

    let input = required_path_arg(context, 0, "input animation path")?;
    let output = required_path_arg(context, 1, "output GIF path")?;
    let (animation, fps, duration, quantizer_speed) = {
        #[cfg(feature = "tracing")]
        let _prepare_animation_span = tracing::debug_span!("prepare_animation").entered();

        let animation = load_animation(&input)?;
        let fps = (context.float_flag("fps").unwrap_or(15.0) as f32).max(1.0);
        let duration = (context.float_flag("duration").unwrap_or(10.0) as f32).max(0.1);
        let quantizer_speed = parse_gif_quantizer_speed(context)?;
        (animation, fps, duration, quantizer_speed)
    };

    println!("render-gif fps={fps:.3} duration={duration:.3}s quantizer_speed={quantizer_speed}");

    let gif = {
        #[cfg(feature = "tracing")]
        let _render_animation_span = tracing::debug_span!("render_animation").entered();

        Renderer::default()
            .render_gif(
                &animation,
                GifRenderConfig::new(
                    RenderConfig::new(Rgba8::TRANSPARENT, 1.0),
                    fps,
                    duration,
                    quantizer_speed,
                ),
            )
            .map_err(|error| to_action_error(&error))?
    };
    {
        #[cfg(feature = "tracing")]
        let _write_output_span = tracing::debug_span!("write_output").entered();

        fs::write(&output, gif).map_err(|error| {
            to_action_error(&format!(
                "failed to write GIF to {}: {error}",
                output.display()
            ))
        })?;
    }
    println!("gif written to {}", output.display());
    Ok(())
}

fn render_png_command(context: &Context) -> ActionResult {
    let input = required_path_arg(context, 0, "input animation path")?;
    let output = required_path_arg(context, 1, "output PNG path")?;
    let animation = load_animation(&input)?;
    let frame = context.float_flag("frame").unwrap_or(0.0) as f32;
    let background = match context.string_flag("background") {
        Ok(value) => parse_rgba8(&value)?,
        Err(FlagError::NotFound) => Rgba8::TRANSPARENT,
        Err(error) => return Err(to_action_error(&error)),
    };
    let scale = parse_render_scale(context)?;

    let raster = Renderer::default()
        .render_frame(&animation, frame, RenderConfig::new(background, scale))
        .map_err(|error| to_action_error(&error))?;
    let image =
        RgbaImage::from_raw(raster.width, raster.height, raster.pixels).ok_or_else(|| {
            to_action_error("failed to convert rendered frame into an RGBA image buffer")
        })?;
    image.save(&output).map_err(|error| {
        to_action_error(&format!(
            "failed to write PNG to {}: {error}",
            output.display()
        ))
    })?;
    println!("png written to {}", output.display());
    Ok(())
}

fn render_mp4_command(context: &Context) -> ActionResult {
    #[cfg(feature = "tracing")]
    let _render_mp4_command_span = tracing::debug_span!("render_mp4_command").entered();

    let input = required_path_arg(context, 0, "input animation path")?;
    let output = required_path_arg(context, 1, "output MP4 path")?;
    let (prepared, timing, render_config, encode_options) = {
        #[cfg(feature = "tracing")]
        let _prepare_animation_span = tracing::debug_span!("prepare_animation").entered();

        let animation = load_animation(&input)?;
        let fps = (context.float_flag("fps").unwrap_or(30.0) as f32).max(1.0);
        let duration = (context.float_flag("duration").unwrap_or(10.0) as f32).max(0.1);
        let background = parse_background_flag(context)?;
        let scale = parse_render_scale(context)?;
        let encode_options = parse_mp4_encode_options(context)?;
        let renderer = Renderer::default();
        let prepared = renderer
            .prepare(&animation)
            .map_err(|error| to_action_error(&error))?;
        let timing = resolve_timing(prepared.animation(), fps, duration);
        (
            prepared,
            timing,
            RenderConfig::new(background, scale),
            encode_options,
        )
    };

    println!(
        "render-mp4 fps={:.3} duration={:.3}s background=#{:02X}{:02X}{:02X}{:02X} scale={:.3} codec={} pix_fmt={} mode={} preset={}",
        timing.target_fps,
        timing.output_duration_seconds,
        render_config.background.r,
        render_config.background.g,
        render_config.background.b,
        render_config.background.a,
        render_config.scale,
        encode_options.codec.ffmpeg_name(),
        encode_options.pixel_format,
        encode_options.describe_quality_mode(),
        encode_options.preset
    );

    let scratch = prepared
        .new_scratch_pixmap_for_config(render_config)
        .map_err(|error| to_action_error(&error))?;
    let width = scratch.width();
    let height = scratch.height();
    let size_arg = format!("{width}x{height}");
    let fps_arg = format!("{:.6}", timing.target_fps);
    let output_arg = output.to_string_lossy().into_owned();
    let quality_args = encode_options.ffmpeg_quality_args();

    let mut child = ProcessCommand::new("ffmpeg")
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "rawvideo",
            "-pixel_format",
            "rgba",
            "-video_size",
            &size_arg,
            "-framerate",
            &fps_arg,
            "-i",
            "-",
            "-an",
            "-c:v",
            encode_options.codec.ffmpeg_name(),
            "-preset",
            &encode_options.preset,
            "-pix_fmt",
            &encode_options.pixel_format,
            "-movflags",
            "+faststart",
        ])
        .args(&quality_args)
        .arg(&output_arg)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            to_action_error(&format!(
                "failed to spawn ffmpeg for MP4 rendering: {error}. Is ffmpeg installed and on PATH?"
            ))
        })?;

    {
        #[cfg(feature = "tracing")]
        let _encode_frames_span = tracing::debug_span!("encode_frames").entered();

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| to_action_error("failed to open ffmpeg stdin"))?;
        let mut scratch = scratch;
        for frame_index in 0..timing.output_frames {
            let source_frame = prepared.animation().in_point
                + (frame_index as f32) * prepared.animation().frame_rate / timing.target_fps;
            if source_frame >= prepared.animation().out_point {
                break;
            }

            prepared
                .render_frame_into_pixmap(source_frame, render_config, &mut scratch)
                .map_err(|error| to_action_error(&error))?;
            if let Err(error) = stdin.write_all(scratch.data()) {
                drop(stdin);
                let ffmpeg_detail = describe_ffmpeg_stream_failure(child, frame_index);
                return Err(to_action_error(&format!(
                    "failed to stream frame {frame_index} to ffmpeg stdin: {error}. {ffmpeg_detail}"
                )));
            }
        }
    }

    let output = {
        #[cfg(feature = "tracing")]
        let _wait_ffmpeg_span = tracing::debug_span!("wait_ffmpeg").entered();

        wait_for_ffmpeg_output(child)?
    };
    if !output.status.success() {
        return Err(to_action_error(&format!(
            "ffmpeg failed while encoding MP4: {}",
            format_ffmpeg_failure(&output)
        )));
    }

    println!("mp4 written to {output_arg}");
    Ok(())
}

fn required_path_arg(context: &Context, index: usize, label: &str) -> Result<PathBuf, ActionError> {
    context
        .args
        .get(index)
        .map(PathBuf::from)
        .ok_or_else(|| to_action_error(&format!("missing {label}")))
}

fn load_animation(path: &PathBuf) -> Result<Animation, ActionError> {
    if path_uses_dotlottie(path) {
        #[cfg(feature = "dotlottie")]
        {
            let bytes = fs::read(path).map_err(|error| {
                to_action_error(&format!("failed to read {}: {error}", path.display()))
            })?;
            return Animation::from_dotlottie_bytes(&bytes)
                .map_err(|error| to_action_error(&error));
        }
        #[cfg(not(feature = "dotlottie"))]
        {
            return Err(to_action_error(
                "`.lottie` input requires building this example with `--features dotlottie`",
            ));
        }
    }

    let json = fs::read_to_string(path)
        .map_err(|error| to_action_error(&format!("failed to read {}: {error}", path.display())))?;
    Animation::from_json_str(&json).map_err(|error| to_action_error(&error))
}

fn path_uses_dotlottie(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("lottie"))
}

fn wait_for_ffmpeg_output(child: Child) -> Result<Output, ActionError> {
    child.wait_with_output().map_err(|error| {
        to_action_error(&format!("failed to wait for ffmpeg MP4 process: {error}"))
    })
}

fn describe_ffmpeg_stream_failure(child: Child, frame_index: usize) -> String {
    match wait_for_ffmpeg_output(child) {
        Ok(output) => format!(
            "ffmpeg terminated while receiving frame {frame_index}: {}",
            format_ffmpeg_failure(&output)
        ),
        Err(error) => format!(
            "ffmpeg also failed to report its exit status after frame {frame_index}: {}",
            error.message
        ),
    }
}

fn format_ffmpeg_failure(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim();
    if stderr.is_empty() {
        format!("exit status {}", output.status)
    } else {
        format!("exit status {}: {}", output.status, stderr)
    }
}

fn parse_background_flag(context: &Context) -> Result<Rgba8, ActionError> {
    match context.string_flag("background") {
        Ok(value) => parse_rgba8(&value),
        Err(FlagError::NotFound) => Ok(Rgba8::TRANSPARENT),
        Err(error) => Err(to_action_error(&error)),
    }
}

fn parse_render_scale(context: &Context) -> Result<f32, ActionError> {
    let scale = match context.float_flag("scale") {
        Ok(value) => value as f32,
        Err(FlagError::NotFound) => 1.0,
        Err(error) => return Err(to_action_error(&error)),
    };
    if !scale.is_finite() || scale <= 0.0 {
        return Err(to_action_error(&format!(
            "invalid scale `{scale}`: expected a positive finite number"
        )));
    }
    Ok(scale)
}

#[cfg(feature = "gif")]
fn parse_gif_quantizer_speed(context: &Context) -> Result<i32, ActionError> {
    let quantizer_speed = match context.int_flag("quantizer-speed") {
        Ok(value) => value,
        Err(FlagError::NotFound) => {
            return Ok(GifRenderConfig::default().color_quantizer_speed);
        }
        Err(error) => return Err(to_action_error(&error)),
    };

    if !(1..=30).contains(&quantizer_speed) {
        return Err(to_action_error(&format!(
            "invalid GIF quantizer speed `{quantizer_speed}`: expected an integer between 1 and 30"
        )));
    }

    i32::try_from(quantizer_speed)
        .map_err(|_| to_action_error("GIF quantizer speed does not fit in i32"))
}

fn parse_mp4_encode_options(context: &Context) -> Result<Mp4EncodeOptions, ActionError> {
    let codec = match context.string_flag("codec") {
        Ok(value) => Mp4Codec::parse(&value)?,
        Err(FlagError::NotFound) => Mp4Codec::Libx264,
        Err(error) => return Err(to_action_error(&error)),
    };

    let lossless = context.bool_flag("lossless");
    let crf = match context.int_flag("crf") {
        Ok(value) => value,
        Err(FlagError::NotFound) => 18,
        Err(error) => return Err(to_action_error(&error)),
    };
    if !(0..=51).contains(&crf) {
        return Err(to_action_error(&format!(
            "invalid CRF `{crf}`: expected an integer between 0 and 51"
        )));
    }
    if lossless && !matches!(context.int_flag("crf"), Err(FlagError::NotFound)) {
        return Err(to_action_error(
            "cannot combine --lossless with --crf, because lossless mode fixes the quantizer automatically",
        ));
    }

    let preset = match context.string_flag("preset") {
        Ok(value) => {
            let preset = value.trim();
            if preset.is_empty() {
                return Err(to_action_error(
                    "invalid preset: expected a non-empty libx264 preset name",
                ));
            }
            preset.to_string()
        }
        Err(FlagError::NotFound) => "medium".to_string(),
        Err(error) => return Err(to_action_error(&error)),
    };

    let pixel_format = match context.string_flag("pix-fmt") {
        Ok(value) => codec.validate_pixel_format(&value)?,
        Err(FlagError::NotFound) => codec.default_pixel_format().to_string(),
        Err(error) => return Err(to_action_error(&error)),
    };

    Ok(Mp4EncodeOptions {
        codec,
        pixel_format,
        quality: if lossless {
            Mp4QualityMode::Lossless
        } else {
            Mp4QualityMode::Crf(crf as u8)
        },
        preset,
    })
}

fn parse_rgba8(value: &str) -> Result<Rgba8, ActionError> {
    let hex = value.trim().trim_start_matches('#');
    match hex.len() {
        6 => {
            let [red, green, blue] = parse_hex_channels::<3>(hex, value)?;
            Ok(Rgba8::new(red, green, blue, 255))
        }
        8 => {
            let [red, green, blue, alpha] = parse_hex_channels::<4>(hex, value)?;
            Ok(Rgba8::new(red, green, blue, alpha))
        }
        _ => Err(to_action_error(&format!(
            "invalid background `{value}`: expected RRGGBB or RRGGBBAA"
        ))),
    }
}

fn parse_hex_channels<const N: usize>(
    hex: &str,
    original_value: &str,
) -> Result<[u8; N], ActionError> {
    let mut channels = [0; N];
    let mut chunks = hex.as_bytes().chunks_exact(2);

    for (channel, chunk) in channels.iter_mut().zip(chunks.by_ref()) {
        let chunk = str::from_utf8(chunk).map_err(|_| {
            to_action_error(&format!(
                "invalid background `{original_value}`: expected ASCII hex digits"
            ))
        })?;
        *channel = parse_hex_channel(chunk)?;
    }

    if !chunks.remainder().is_empty() {
        return Err(to_action_error(&format!(
            "invalid background `{original_value}`: expected RRGGBB or RRGGBBAA"
        )));
    }

    Ok(channels)
}

fn parse_hex_channel(value: &str) -> Result<u8, ActionError> {
    u8::from_str_radix(value, 16).map_err(|error| {
        to_action_error(&format!(
            "invalid hex channel `{value}` in background color: {error}"
        ))
    })
}

fn to_action_error(error: &(impl ToString + ?Sized)) -> ActionError {
    ActionError {
        message: error.to_string(),
    }
}

fn resolve_timing(
    animation: &Animation,
    requested_fps: f32,
    requested_duration: f32,
) -> RenderTiming {
    let source_fps = animation.frame_rate.max(1.0);
    let target_fps = source_fps.min(requested_fps.max(1.0));
    let max_duration_seconds = animation
        .duration_seconds()
        .min(requested_duration.max(0.1));
    let output_frames = (target_fps * max_duration_seconds).floor().max(1.0) as usize;

    RenderTiming {
        target_fps,
        output_frames,
        output_duration_seconds: max_duration_seconds,
    }
}

fn collect_animation_stats(animation: &Animation) -> AnimationStats {
    let mut stats = AnimationStats {
        top_level_layers: animation.layers.len(),
        assets_total: animation.assets.len(),
        ..AnimationStats::default()
    };

    for layer in &animation.layers {
        collect_layer_stats(layer, &mut stats);
    }

    for asset in &animation.assets {
        collect_asset_stats(asset, &mut stats);
    }

    stats
}

fn collect_asset_stats(asset: &Asset, stats: &mut AnimationStats) {
    if !asset.layers.is_empty() {
        stats.precomp_assets += 1;
        stats.asset_layer_definitions += asset.layers.len();
        for layer in &asset.layers {
            collect_layer_stats(layer, stats);
        }
    }

    if asset.is_image_asset() {
        stats.image_assets += 1;
        if asset.is_embedded_image_asset() {
            stats.embedded_image_assets += 1;
        } else {
            stats.external_image_assets += 1;
        }
    }
}

fn collect_layer_stats(layer: &Layer, stats: &mut AnimationStats) {
    stats.total_layer_definitions += 1;
    increment_count(&mut stats.layer_type_counts, layer.layer_type.name());

    if layer.hidden {
        stats.hidden_layers += 1;
    }
    if layer.parent.is_some() {
        stats.parented_layers += 1;
    }
    if !layer.masks.is_empty() {
        stats.masked_layers += 1;
        stats.masks_total += layer.masks.len();
    }
    if layer.track_matte_mode().is_some() {
        stats.track_matte_layers += 1;
    }
    if layer.is_matte_source_layer() {
        stats.matte_source_layers += 1;
    }
    if !layer.effects.is_empty() {
        stats.effects_layers += 1;
        stats.effects_total += layer.effects.len();
    }
    if layer.time_remap.is_some() {
        stats.time_remap_layers += 1;
    }
    if let Some(text) = layer.text.as_ref() {
        if text.has_animators() {
            stats.text_layers_with_animators += 1;
        }
        if text.has_path() {
            stats.text_layers_with_path += 1;
        }
    }

    collect_shape_items(&layer.shapes, stats);
}

fn collect_shape_items(items: &[ShapeItem], stats: &mut AnimationStats) {
    for item in items {
        stats.shape_items_total += 1;
        increment_count(&mut stats.shape_item_type_counts, &item.item_type);
        if item.item_type == "gr" {
            collect_shape_items(&item.items, stats);
        }
    }
}

fn increment_count(counts: &mut BTreeMap<String, usize>, key: &str) {
    *counts.entry(key.to_string()).or_default() += 1;
}

fn format_counts(counts: &BTreeMap<String, usize>) -> String {
    if counts.is_empty() {
        return "none".to_string();
    }

    counts
        .iter()
        .map(|(name, count)| format!("{name}={count}"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use rasterlottie::Rgba8;

    use super::{parse_rgba8, path_uses_dotlottie, to_action_error};

    #[test]
    fn parse_rgba8_accepts_rgb_hex() {
        assert_eq!(
            parse_rgba8("#112233").unwrap(),
            Rgba8::new(0x11, 0x22, 0x33, 0xff)
        );
    }

    #[test]
    fn parse_rgba8_accepts_rgba_hex() {
        assert_eq!(
            parse_rgba8("11223344").unwrap(),
            Rgba8::new(0x11, 0x22, 0x33, 0x44)
        );
    }

    #[test]
    fn parse_rgba8_rejects_non_ascii_without_panicking() {
        let error = parse_rgba8("あaaaa").unwrap_err();
        assert_eq!(
            error.message,
            to_action_error("invalid background `あaaaa`: expected ASCII hex digits").message
        );
    }

    #[test]
    fn parse_rgba8_rejects_invalid_length() {
        let error = parse_rgba8("12345").unwrap_err();
        assert_eq!(
            error.message,
            to_action_error("invalid background `12345`: expected RRGGBB or RRGGBBAA").message
        );
    }

    #[test]
    fn path_uses_dotlottie_matches_case_insensitive_extension() {
        assert!(path_uses_dotlottie(std::path::Path::new("demo.lottie")));
        assert!(path_uses_dotlottie(std::path::Path::new("demo.LOTTIE")));
        assert!(!path_uses_dotlottie(std::path::Path::new("demo.json")));
    }
}
