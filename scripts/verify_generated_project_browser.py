#!/usr/bin/env python3
"""Verify that an unchanged `kael new` project runs its packaged wasm app."""

from __future__ import annotations

import argparse
import base64
import importlib.metadata
import json
from pathlib import Path
from typing import Any

from playwright.sync_api import sync_playwright


PLAYWRIGHT_VERSION = "1.62.0"


class VerificationFailure(RuntimeError):
    """The generated project did not satisfy its browser contract."""


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--url", required=True)
    parser.add_argument("--artifacts", type=Path, required=True)
    parser.add_argument("--metadata", type=Path, required=True)
    parser.add_argument("--workspace", type=Path, required=True)
    parser.add_argument("--main-sha256", required=True)
    return parser.parse_args()


def require(condition: bool, message: str) -> None:
    if not condition:
        raise VerificationFailure(message)


def verify_local_patches(metadata_path: Path, workspace: Path) -> dict[str, str]:
    metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
    packages = metadata.get("packages", [])
    expected = {
        "kael": workspace / "crates" / "kael" / "Cargo.toml",
        "kael_ui": workspace / "crates" / "kael_ui" / "Cargo.toml",
    }
    result: dict[str, str] = {}
    for name, manifest in expected.items():
        candidates = [package for package in packages if package.get("name") == name]
        require(len(candidates) == 1, f"metadata did not resolve exactly one {name} package")
        package = candidates[0]
        actual = Path(package["manifest_path"]).resolve()
        require(package.get("source") is None, f"{name} resolved from a registry: {package}")
        require(
            actual == manifest.resolve(),
            f"{name} did not resolve to the local workspace: {actual}",
        )
        result[name] = str(actual)
    return result


def frame_evidence(page: Any) -> dict[str, Any]:
    return page.evaluate(
        """() => {
          const canvas = document.querySelector("#blade");
          if (!(canvas instanceof HTMLCanvasElement)) {
            throw new Error("generated app did not create #blade canvas");
          }
          if (!canvas.getContext("webgl2")) {
            throw new Error("generated app did not retain a WebGL2 context");
          }
          const bounds = canvas.getBoundingClientRect();
          return {
            css: { width: bounds.width, height: bounds.height },
            width: canvas.width,
            height: canvas.height,
            frame: canvas.dataset.kaelFrame ?? null,
            frameCount: Number(canvas.dataset.kaelFrameCount ?? 0),
          };
        }"""
    )


def screenshot_pixel_evidence(page: Any, png: bytes) -> dict[str, int]:
    source = "data:image/png;base64," + base64.b64encode(png).decode("ascii")
    return page.evaluate(
        """async (source) => {
          const image = new Image();
          image.src = source;
          await image.decode();
          const canvas = document.createElement("canvas");
          canvas.width = image.naturalWidth;
          canvas.height = image.naturalHeight;
          const context = canvas.getContext("2d", { willReadFrequently: true });
          if (!context) throw new Error("screenshot 2D context is unavailable");
          context.drawImage(image, 0, 0);
          const pixels = context.getImageData(0, 0, canvas.width, canvas.height).data;
          const baseline = pixels.slice(0, 4);
          let differingPixels = 0;
          let minimumLuma = 255;
          let maximumLuma = 0;
          for (let index = 0; index < pixels.length; index += 4) {
            const differs =
              pixels[index] !== baseline[0] ||
              pixels[index + 1] !== baseline[1] ||
              pixels[index + 2] !== baseline[2] ||
              pixels[index + 3] !== baseline[3];
            if (differs) differingPixels += 1;
            const luma = Math.round(
              pixels[index] * 0.2126 +
              pixels[index + 1] * 0.7152 +
              pixels[index + 2] * 0.0722
            );
            minimumLuma = Math.min(minimumLuma, luma);
            maximumLuma = Math.max(maximumLuma, luma);
          }
          return {
            width: canvas.width,
            height: canvas.height,
            differingPixels,
            lumaRange: maximumLuma - minimumLuma,
          };
        }""",
        source,
    )


def main() -> int:
    args = parse_args()
    args.artifacts.mkdir(parents=True, exist_ok=True)
    require(
        importlib.metadata.version("playwright") == PLAYWRIGHT_VERSION,
        f"Playwright {PLAYWRIGHT_VERSION} is required",
    )
    local_patches = verify_local_patches(args.metadata, args.workspace)
    diagnostics: dict[str, list[str]] = {
        "console": [],
        "page_errors": [],
        "request_failures": [],
    }
    with sync_playwright() as playwright:
        browser = playwright.chromium.launch(
            headless=True,
            args=[
                "--enable-webgl",
                "--enable-unsafe-swiftshader",
                "--use-angle=swiftshader",
            ],
        )
        context = browser.new_context(viewport={"width": 1280, "height": 800})
        page = context.new_page()
        page.on(
            "console",
            lambda message: diagnostics["console"].append(
                f"{message.type}: {message.text}"
            ),
        )
        page.on("pageerror", lambda error: diagnostics["page_errors"].append(str(error)))
        page.on(
            "requestfailed",
            lambda request: diagnostics["request_failures"].append(
                f"{request.method} {request.url}: {request.failure or 'failed'}"
            ),
        )
        page.goto(args.url, wait_until="domcontentloaded", timeout=30_000)
        page.wait_for_function(
            """() =>
              document.documentElement.dataset.kaelReady === "true" &&
              document.querySelector("#blade")?.dataset.kaelFrame === "presented" &&
              Number(document.querySelector("#blade")?.dataset.kaelFrameCount ?? 0) >= 2
            """,
            timeout=30_000,
        )
        error_text = page.locator("#kael-error").inner_text().strip()
        require(not error_text, f"generated app reported a startup error: {error_text}")
        frame = frame_evidence(page)
        require(frame["frame"] == "presented", "generated app did not present a frame")
        require(frame["frameCount"] >= 2, "generated app presented too few frames")
        screenshot = args.artifacts / "generated-project-browser.png"
        png = page.screenshot(path=screenshot, full_page=True)
        pixels = screenshot_pixel_evidence(page, png)
        require(
            pixels["differingPixels"] >= 64,
            f"generated page lacked retained composited content: {pixels}",
        )
        require(pixels["lumaRange"] >= 8, f"generated page was visually blank: {pixels}")
        require(
            abs(frame["css"]["width"] - 1280) <= 1
            and abs(frame["css"]["height"] - 800) <= 1,
            f"generated canvas did not fill the viewport: {frame['css']}",
        )
        require(not diagnostics["page_errors"], f"page errors: {diagnostics['page_errors']}")
        require(
            not diagnostics["request_failures"],
            f"request failures: {diagnostics['request_failures']}",
        )
        deprecated_init = [
            message
            for message in diagnostics["console"]
            if "deprecated parameters for the initialization function" in message
        ]
        require(
            not deprecated_init,
            f"generated loader used deprecated wasm-bindgen initialization: {deprecated_init}",
        )
        version = browser.version
        context.close()
        browser.close()

    report = {
        "status": "passed",
        "browser": {"engine": "chromium", "version": version},
        "main_sha256": args.main_sha256,
        "local_patches": local_patches,
        "frame": frame,
        "pixels": pixels,
        "diagnostics": diagnostics,
        "screenshot": screenshot.name,
    }
    temporary = args.artifacts / "report.json.tmp"
    temporary.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    temporary.replace(args.artifacts / "report.json")
    print(f"Generated project browser proof passed: {args.artifacts / 'report.json'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
