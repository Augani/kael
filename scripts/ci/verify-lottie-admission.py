#!/usr/bin/env python3
"""Require Lottie fork admission tests and the isolated native loader default policy."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[2]
PORTABLE_TESTS = (
    "prepared_choker_numeric_limits_render_tiny_frames",
    "admission_archive_limits_are_inclusive_and_ignore_unselected_contents",
    "admission_charges_each_repeated_image_embedding",
    "admission_prepared_policy_guards_late_images_without_charging_cache_hits",
    "admission_preserves_crc_and_forged_size_errors",
    "admission_preserves_zip64_entry_headers_and_archive_comments",
    "admission_public_policy_bounds_cross_aspect_work_and_preserves_unused_lazy_assets",
    "admission_public_policy_includes_triangle_sampling_weights",
    "admission_rejects_duplicate_central_directory_names",
    "admission_rejects_manifest_expansion_before_parsing",
    "admission_rejects_oversized_image_target_before_resize",
    "malformed_and_truncated_archives_return_errors",
    "stored_and_deflated_dotlottie_render_archive_image_pixels",
    "stored_and_deflated_dotlottie_render_embedded_glyph_text_pixels",
    "stored_and_deflated_manifest_select_the_initial_animation",
)
NATIVE_TESTS = PORTABLE_TESTS + (
    "choker_finite_boundaries_match_geometric_alpha_oracle",
    "prepared_choker_boundaries_preserve_small_asymmetric_pixels",
    "public_prepared_frame_transfers_canvas_without_duplicate_allocation",
    "bounded_reader_rejects_headers_without_reading_and_limits_actual_output",
    "admission_checks_encoded_native_rgba_target_and_aggregate_before_pixels",
    "admission_checks_actual_header_and_triangle_cross_aspect_scratch",
    "admission_includes_triangle_weight_buffers",
    "admission_cache_hits_and_failures_preserve_exact_retained_accounting",
    "admission_rolls_back_after_pixel_decode_failure",
    "admission_base64_boundary_and_declared_target_precede_decode",
    "admission_rejects_resolver_bytes_before_retention",
    "target_corpus_supports_and_renders_each_animation",
    "reference_goldens_match_with_small_pixel_error",
)
CHROME_TESTS = PORTABLE_TESTS + (
    "admission_raised_limits_still_reject_unrepresentable_target_buffers",
)
DEFAULT_LOADER_POLICY_TEST = (
    "public_loader_admission_regression::"
    "public_loader_starts_two_of_three_across_apps_and_direct_constructors_still_work"
)
SUMMARY = re.compile(
    r"^test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;[^\n]*$",
    re.MULTILINE,
)


def validate_runtime(text, backend):
    """Require executed regressions, including suites that would otherwise pass empty."""
    reports = SUMMARY.findall(text)
    expected_suites = 6 if backend == "native" else 1
    if len(reports) != expected_suites:
        raise RuntimeError(
            f"{backend}: expected {expected_suites} successful test summaries, got {len(reports)}"
        )
    suites = [tuple(map(int, report)) for report in reports]
    if any(passed == 0 or failed or ignored for passed, failed, ignored in suites):
        raise RuntimeError(f"{backend}: every suite must execute tests with zero failures/ignored")
    if re.search(r"\b(?:skipped|skipping)\b", text, re.IGNORECASE):
        raise RuntimeError(f"{backend}: runtime reported skipped tests")
    if any(int(count) for count in re.findall(r"(\d+) filtered out", text)):
        raise RuntimeError(f"{backend}: a filtered test suite is not complete")
    minimums = (119, 1, 1, 1, 16, 2) if backend == "native" else (17,)
    if any(passed < minimum for (passed, _, _), minimum in zip(suites, minimums)):
        raise RuntimeError(f"{backend}: expected suite pass counts >= {minimums}, got {suites}")
    required = NATIVE_TESTS if backend == "native" else CHROME_TESTS
    for name in required:
        # Stop at the next test header: another test's verdict cannot satisfy this one.
        verdict = (r"^test (?:[^\s:]+::)*" + re.escape(name)
                   + r" \.\.\.(?:(?!\ntest ).)*?\bok(?:\r?\n|$)")
        if len(re.findall(verdict, text, re.MULTILINE | re.DOTALL)) != 1:
            raise RuntimeError(f"{backend}: missing or repeated mandatory regression {name}")
    if backend == "chrome" and "Running headless tests in Chrome on " not in text:
        raise RuntimeError("chrome: runner did not confirm actual headless Chrome execution")
    return [report[0] for report in suites]


def validate_default_loader_policy(text):
    """Require the sole native public-policy test, never an empty/filtered harness."""
    if SUMMARY.findall(text) != [("1", "0", "0")]:
        raise RuntimeError("native default loader policy: expected exactly 1 passed, 0 failed/ignored")
    if re.findall(r"^running (\d+) tests?$", text, re.MULTILINE) != ["1"]:
        raise RuntimeError("native default loader policy: expected one executed test harness")
    if any(int(count) for count in re.findall(r"(\d+) filtered out", text)):
        raise RuntimeError("native default loader policy: filtered tests are not complete")
    verdict = (r"^test " + re.escape(DEFAULT_LOADER_POLICY_TEST)
               + r" \.\.\.(?:(?!\ntest ).)*?\bok(?:\r?\n|$)")
    if len(re.findall(verdict, text, re.MULTILINE | re.DOTALL)) != 1:
        raise RuntimeError("native default loader policy: missing/repeated public-policy verdict")
    return 1


def run(command, log, env):
    print("Running:", " ".join(command), flush=True)
    with log.open("w") as output:
        result = subprocess.run(
            command, cwd=ROOT, env=env, stdout=output, stderr=subprocess.STDOUT,
            text=True, timeout=1200,
        )
    text = log.read_text()
    print(text, end="", flush=True)
    if result.returncode:
        raise RuntimeError(f"{log}: command failed with exit {result.returncode}")
    return text


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(64 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def browser_artifact(messages):
    manifest = (ROOT / "vendor/rasterlottie/Cargo.toml").resolve()
    source = (ROOT / "vendor/rasterlottie/tests/zip9_compatibility.rs").resolve()
    artifacts = set()
    for message in messages:
        if (message.get("reason") == "compiler-artifact"
                and message.get("package_id", "").rsplit("#", 1)[-1].split("@", 1)[0]
                == "kael_rasterlottie"
                and Path(message["manifest_path"]).resolve() == manifest
                and message["target"]["name"] == "zip9_compatibility"
                and Path(message["target"]["src_path"]).resolve() == source
                and message["target"]["kind"] == ["test"]
                and message["profile"]["test"]):
            for name in message["filenames"]:
                if name.endswith(".wasm"):
                    artifact = Path(name).resolve(strict=True)
                    if not artifact.is_file():
                        raise RuntimeError(f"Browser test artifact is not a regular file: {artifact}")
                    artifacts.add(artifact)
    if len(artifacts) != 1:
        raise RuntimeError(f"Expected one fork-owned Cargo browser test artifact, got {artifacts}")
    return artifacts.pop()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", choices=("native", "chrome"), required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    args = parser.parse_args()
    evidence = args.evidence.resolve()
    evidence.mkdir(parents=True, exist_ok=True)
    # A failed rerun must not leave a previous success record in its evidence directory.
    (evidence / "result.json").unlink(missing_ok=True)
    env = os.environ.copy()
    env["CARGO_TERM_COLOR"] = "never"
    commands = []
    result = {"schema_version": 1, "backend": args.backend}
    if args.backend == "native":
        command = ["cargo", "test", "--locked", "-p", "kael_rasterlottie", "--all-features",
                   "--", "--nocapture"]
    else:
        build = ["cargo", "test", "--locked", "-p", "kael_rasterlottie", "--no-default-features",
                 "--features", "dotlottie,images,text", "--target", "wasm32-unknown-unknown",
                 "--test", "zip9_compatibility", "--no-run", "--message-format=json"]
        commands.append(build)
        (evidence / "commands.json").write_text(json.dumps(commands, indent=2) + "\n")
        print("Building:", " ".join(build), flush=True)
        with (evidence / "browser-build.jsonl").open("w") as output, \
                (evidence / "browser-build.log").open("w") as errors:
            built = subprocess.run(build, cwd=ROOT, env=env, stdout=output, stderr=errors, timeout=1200)
        if built.returncode:
            print((evidence / "browser-build.log").read_text(), file=sys.stderr)
            raise RuntimeError("Chrome Lottie test harness failed to build")
        messages = [json.loads(line) for line in
                    (evidence / "browser-build.jsonl").read_text().splitlines()]
        artifact = browser_artifact(messages)
        result.update({"wasm_path": str(artifact), "wasm_sha256": sha256(artifact)})
        env["WASM_BINDGEN_TEST_TIMEOUT"] = "60"
        command = ["wasm-bindgen-test-runner", str(artifact), "--nocapture"]
    commands.append(command)
    (evidence / "commands.json").write_text(json.dumps(commands, indent=2) + "\n")
    log = evidence / f"{args.backend}-runtime.log"
    text = run(command, log, env)
    result.update({"suite_passes": validate_runtime(text, args.backend),
                   "runtime_log_sha256": sha256(log)})
    result["total_passed"] = sum(result["suite_passes"])
    if args.backend == "native":
        # A separate Cargo integration-test process exercises the real default
        # singleton without parallel library tests occupying its two slots.
        policy_command = ["cargo", "test", "--locked", "-p", "kael", "--test",
                          "lottie_loader_policy", "--features", "lottie,test-support",
                          "--", "--test-threads=1", "--nocapture"]
        commands.append(policy_command)
        (evidence / "commands.json").write_text(json.dumps(commands, indent=2) + "\n")
        policy_env = env.copy()
        policy_env["KAEL_HEADLESS"] = "1"
        policy_log = evidence / "native-default-loader-policy.log"
        policy_text = run(policy_command, policy_log, policy_env)
        result["default_loader_policy"] = {
            "passed": validate_default_loader_policy(policy_text),
            "test_name": DEFAULT_LOADER_POLICY_TEST,
            "runtime_log_sha256": sha256(policy_log),
            "integration_source_sha256": sha256(
                ROOT / "crates/kael/tests/lottie_loader_policy.rs"),
            "core_loader_source_sha256": sha256(ROOT / "crates/kael/src/lottie.rs"),
            "KAEL_HEADLESS": "1",
        }
    (evidence / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(f"LOTTIE_ADMISSION_RUNTIME_OK: backend={args.backend} passed={result['total_passed']}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, ValueError, subprocess.TimeoutExpired) as error:
        print(f"Lottie admission verification failed: {error}", file=sys.stderr)
        sys.exit(1)
