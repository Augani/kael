#!/usr/bin/env python3
"""Run Kael's packaged browser proofs in Playwright's three browser engines."""

from __future__ import annotations

import argparse
import base64
import json
import os
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path
from typing import Any
from urllib.parse import parse_qs, urlparse

from playwright.sync_api import Browser, BrowserType, Page, Playwright, sync_playwright


ENGINES = ("chromium", "firefox", "webkit")
DESKTOP_VIEWPORT = {"width": 1280, "height": 800}
NARROW_VIEWPORT = {"width": 430, "height": 720}
SUITE_COMPACT_VIEWPORT = {"width": 760, "height": 720}
SUITE_WIDE_BEACON = (
    "__kael_suite_pass__=1&rows=1000000&columns=16384&blocks=250000"
    "&slides=10000&shapes=100000&selection=anchor_focus&pointer=passed"
    "&windows=passed&routes=passed&mounts=bounded&export=png"
)
SUITE_COMPACT_BEACON = "__kael_suite_compact_pass__=1&layout=compact&mounts=bounded"
WEBSOCKET_BEACON = (
    "__kael_websocket_pass__=1&protocol=passed&text=passed&binary=passed"
    "&ordered=passed&close=passed&error=passed&cancellation=passed"
    "&backpressure=passed&policy=passed&size=passed&reconnect=passed"
)
CAPTURE_BEACON = (
    "__kael_capture_pass__=1&enumeration=passed&start=passed&frames=passed"
    "&lifecycle=passed&bounds=passed&async_error=passed"
)


class VerificationFailure(RuntimeError):
    """One browser failed a release assertion."""


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-url", required=True)
    parser.add_argument("--artifacts", type=Path, required=True)
    parser.add_argument("--workspace", type=Path, required=True)
    parser.add_argument("--echo-server", type=Path)
    parser.add_argument(
        "--engines",
        default=",".join(ENGINES),
        help="comma-separated subset of chromium,firefox,webkit",
    )
    parser.add_argument("--skip-suite", action="store_true")
    parser.add_argument("--skip-realtime", action="store_true")
    parser.add_argument("--skip-capture", action="store_true")
    return parser.parse_args()


def require(condition: bool, message: str) -> None:
    if not condition:
        raise VerificationFailure(message)


def dataset(page: Page) -> dict[str, str]:
    return page.evaluate(
        """() => Object.fromEntries(
          [...document.documentElement.attributes]
            .filter((attribute) => attribute.name.startsWith("data-kael-"))
            .map((attribute) => [attribute.name, attribute.value])
        )"""
    )


def canvas_evidence(page: Page) -> list[dict[str, Any]]:
    return page.evaluate(
        """() => [...document.querySelectorAll("canvas")].map((canvas) => {
          const bounds = canvas.getBoundingClientRect();
          return {
            id: canvas.id,
            primary: canvas.dataset.kaelWindowPrimary ?? null,
            surfaceId: canvas.dataset.kaelWindowSurfaceId ?? null,
            frame: canvas.dataset.kaelFrame ?? null,
            frameCount: Number(canvas.dataset.kaelFrameCount ?? 0),
            pixelReadback: canvas.dataset.kaelPixelReadback ?? null,
            pixelChanged: Number(canvas.dataset.kaelPixelChanged ?? 0),
            pixelLumaRange: Number(canvas.dataset.kaelPixelLumaRange ?? 0),
            pixelHash: canvas.dataset.kaelPixelHash ?? null,
            contextRecovery: canvas.dataset.kaelContextRecovery ?? null,
            contextRecoveryHash: canvas.dataset.kaelContextRecoveryHash ?? null,
            frameDamage: canvas.dataset.kaelFrameDamage ?? null,
            frameDamageRatio: Number(canvas.dataset.kaelFrameDamageRatio ?? 1),
            width: canvas.width,
            height: canvas.height,
            css: {
              left: bounds.left,
              top: bounds.top,
              width: bounds.width,
              height: bounds.height,
            },
          };
        })"""
    )


def capture_retained_page_screenshot(page: Page, path: Path) -> None:
    """Capture the retained frame without depending on WebGL compositor screenshots.

    Firefox under Xvfb can omit a restored WebGL surface from a full-page screenshot
    even though readPixels returns the verified frame. Inserting its exact PNG directly
    beneath the live canvas makes that missing compositor plane harmless while keeping
    DOM-hosted WebViews in their real layer above it.
    """
    data_url = page.evaluate(
        """async () => {
          const canvas = document.querySelector("#blade");
          if (!(canvas instanceof HTMLCanvasElement)) {
            throw new Error("Kael retained canvas was unavailable for evidence capture");
          }
          const png = canvas.toDataURL("image/png");
          if (!png.startsWith("data:image/png;base64,")) {
            throw new Error("Kael retained canvas did not produce PNG evidence");
          }
          const bounds = canvas.getBoundingClientRect();
          const computed = getComputedStyle(canvas);
          const image = document.createElement("img");
          image.id = "kael-retained-evidence-frame";
          image.alt = "";
          image.src = png;
          image.style.position = "fixed";
          image.style.left = `${bounds.left}px`;
          image.style.top = `${bounds.top}px`;
          image.style.width = `${bounds.width}px`;
          image.style.height = `${bounds.height}px`;
          image.style.margin = "0";
          image.style.padding = "0";
          image.style.border = "0";
          image.style.objectFit = "fill";
          image.style.pointerEvents = "none";
          image.style.zIndex = computed.zIndex;
          canvas.parentNode.insertBefore(image, canvas);
          if (typeof image.decode === "function") await image.decode();
          await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
          return png;
        }"""
    )
    # Retain the raw framebuffer alongside the page screenshot. This provides a
    # directly inspectable artifact if a future browser compositor regresses.
    prefix = "data:image/png;base64,"
    framebuffer_path = path.with_name(f"{path.stem}-framebuffer.png")
    framebuffer_path.write_bytes(base64.b64decode(data_url[len(prefix) :], validate=True))
    page.screenshot(path=path, full_page=True)


def capability_snapshot(page: Page) -> dict[str, Any]:
    return page.evaluate(
        """() => {
          const constructible = (factory) => {
            try { factory(); return true; } catch (_) { return false; }
          };
          const probe = document.createElement("canvas");
          const transfer = constructible(() => new DataTransfer());
          const clipboardPayload = transfer && constructible(() => {
            const data = new DataTransfer();
            data.setData("text/plain", "kael");
            const event = new ClipboardEvent("paste", { clipboardData: data });
            if (event.clipboardData?.getData("text/plain") !== "kael") throw new Error();
          });
          return {
            userAgent: navigator.userAgent,
            webgl2: Boolean(probe.getContext("webgl2")),
            webAssembly: typeof WebAssembly === "object",
            modules: "noModule" in HTMLScriptElement.prototype,
            pointerEvents: typeof PointerEvent === "function",
            compositionEvents: typeof CompositionEvent === "function",
            inputEvents: typeof InputEvent === "function",
            resizeObserver: typeof ResizeObserver === "function",
            mutationObserver: typeof MutationObserver === "function",
            file: typeof File === "function",
            fileReader: typeof FileReader === "function",
            blob: typeof Blob === "function",
            dataTransferConstructor: transfer,
            syntheticClipboardPayload: clipboardPayload,
            history: typeof history.pushState === "function",
            canvasBlobExport: typeof HTMLCanvasElement.prototype.toBlob === "function",
            sandboxedIframe: "sandbox" in HTMLIFrameElement.prototype,
            asyncClipboard: Boolean(navigator.clipboard),
            fileSystemAccessPicker: typeof window.showOpenFilePicker === "function",
            webShare: typeof navigator.share === "function",
            notificationApi: typeof window.Notification === "function",
          };
        }"""
    )


def wait_for_terminal_marker(page: Page, expression: str, timeout: int) -> None:
    page.wait_for_function(expression, timeout=timeout)


def page_failure(page: Page) -> str:
    failure = page.locator("#failure")
    if failure.count() == 0:
        return ""
    return failure.inner_text().strip()


def attach_diagnostics(page: Page, diagnostics: dict[str, list[str]]) -> None:
    page.on(
        "console",
        lambda message: diagnostics["console"].append(f"{message.type}: {message.text}"),
    )
    page.on("pageerror", lambda error: diagnostics["page_errors"].append(str(error)))
    page.on(
        "requestfailed",
        lambda request: diagnostics["request_failures"].append(
            f"{request.method} {request.url}: {request.failure or 'failed'}"
        ),
    )
    page.on(
        "request",
        lambda request: diagnostics["beacons"].append(request.url)
        if "__kael_" in request.url
        else None,
    )


def assert_clean_runtime_diagnostics(
    page: Page,
    diagnostics: dict[str, list[str]],
    label: str,
    expected_console_errors: tuple[str, ...] = (),
) -> None:
    # Browser console and page-error delivery is asynchronous relative to the
    # action that triggered it. Give the host one task turn before declaring a
    # smoke run clean so a Rust/Wasm panic cannot race the report writer.
    page.wait_for_timeout(50)
    assert_recorded_runtime_diagnostics(diagnostics, label, expected_console_errors)


def assert_recorded_runtime_diagnostics(
    diagnostics: dict[str, list[str]],
    label: str,
    expected_console_errors: tuple[str, ...] = (),
) -> None:
    require(
        not diagnostics["page_errors"],
        f"{label} reported page errors: {diagnostics['page_errors']}",
    )
    fatal_console = [
        message
        for message in diagnostics["console"]
        if message.startswith("error:") or "panicked at" in message
        if not any(expected in message for expected in expected_console_errors)
    ]
    require(
        not fatal_console,
        f"{label} reported fatal console diagnostics: {fatal_console}",
    )


def wait_for_beacon(
    page: Page,
    diagnostics: dict[str, list[str]],
    marker: str,
    label: str,
) -> None:
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if any(marker in url for url in diagnostics["beacons"]):
            return
        page.wait_for_timeout(25)
    raise VerificationFailure(f"{label} did not publish its semantic pass beacon")


def assert_required_capabilities(capabilities: dict[str, Any], engine: str) -> None:
    required = (
        "webgl2",
        "webAssembly",
        "modules",
        "pointerEvents",
        "compositionEvents",
        "inputEvents",
        "resizeObserver",
        "mutationObserver",
        "file",
        "fileReader",
        "blob",
        "history",
        "canvasBlobExport",
        "sandboxedIframe",
    )
    missing = [name for name in required if not capabilities.get(name)]
    require(not missing, f"{engine} is missing required browser capabilities: {', '.join(missing)}")


def assert_primary_canvas(page: Page, canvases: list[dict[str, Any]], label: str) -> None:
    require(canvases, f"{label} created no retained canvas")
    canvas = next((item for item in canvases if item["id"] == "blade"), canvases[0])
    require(canvas["frame"] == "presented", f"{label} did not present a retained frame")
    require(canvas["frameCount"] >= 2, f"{label} presented too few frames")
    require(canvas["pixelReadback"] == "verified", f"{label} pixel readback was not verified")
    require(canvas["pixelChanged"] >= 64, f"{label} framebuffer contained too little variation")
    require(canvas["pixelLumaRange"] >= 8, f"{label} framebuffer luma range was too small")
    require(
        isinstance(canvas["pixelHash"], str) and len(canvas["pixelHash"]) == 16,
        f"{label} did not expose a framebuffer hash",
    )
    viewport = page.viewport_size
    if viewport:
        require(
            abs(canvas["css"]["width"] - viewport["width"]) <= 1
            and abs(canvas["css"]["height"] - viewport["height"]) <= 1,
            f"{label} canvas did not fill the viewport: {canvas['css']} vs {viewport}",
        )
    layout = page.evaluate(
        """() => ({
          innerWidth,
          innerHeight,
          bodyWidth: document.body.scrollWidth,
          bodyHeight: document.body.scrollHeight,
          failure: document.querySelector("#failure")?.textContent?.trim() ?? "",
        })"""
    )
    require(not layout["failure"], f"{label} reported an in-page failure: {layout['failure']}")
    require(
        layout["bodyWidth"] <= layout["innerWidth"] + 1
        and layout["bodyHeight"] <= layout["innerHeight"] + 1,
        f"{label} leaked page-level overflow: {layout}",
    )


def exercise_file_apis(page: Page, engine: str, output_dir: Path) -> dict[str, Any]:
    result: dict[str, Any] = {}
    import_button = page.get_by_role("button", name="Import bytes")
    require(import_button.count() == 1, f"{engine} did not expose the portable file-picker action")
    with page.expect_file_chooser(timeout=10_000) as chooser_info:
        import_button.click(force=True)
    chooser_info.value.set_files(
        {"name": "kael-matrix.txt", "mimeType": "text/plain", "buffer": b"kael matrix"}
    )
    page.wait_for_function(
        "document.documentElement.dataset.kaelFilePicker === 'passed'", timeout=10_000
    )
    result["picker"] = dataset(page).get("data-kael-file-picker")

    export_button = page.get_by_role("button", name="Export Blob")
    require(export_button.count() == 1, f"{engine} did not expose the portable export action")
    with page.expect_download(timeout=10_000) as download_info:
        export_button.click(force=True)
    download = download_info.value
    download_path = output_dir / f"{engine}-{download.suggested_filename}"
    download.save_as(download_path)
    page.wait_for_function(
        "document.documentElement.dataset.kaelFileExport === 'passed'", timeout=10_000
    )
    result.update(
        {
            "export": dataset(page).get("data-kael-file-export"),
            "download": download_path.name,
            "download_bytes": download_path.stat().st_size,
        }
    )

    drop_result = page.evaluate(
        """() => {
          try {
            const transfer = new DataTransfer();
            transfer.items.add(new File(["kael drop"], "kael-drop.txt", { type: "text/plain" }));
            const canvas = document.querySelector("#blade");
            for (const type of ["dragenter", "dragover", "drop"]) {
              canvas.dispatchEvent(new DragEvent(type, {
                bubbles: true,
                cancelable: true,
                dataTransfer: transfer,
                clientX: 12,
                clientY: 12,
              }));
            }
            return { attempted: true };
          } catch (error) {
            return { attempted: false, reason: String(error) };
          }
        }"""
    )
    result["drop_automation"] = drop_result
    if drop_result["attempted"]:
        page.wait_for_function(
            "document.documentElement.dataset.kaelFileDrop === 'passed'", timeout=10_000
        )
        result["drop"] = dataset(page).get("data-kael-file-drop")
    else:
        result["drop"] = "automation-unavailable"
    return result


def exercise_semantic_input(page: Page, engine: str) -> dict[str, Any]:
    button = page.get_by_role("button", name="Test pointer, text, and animation")
    require(button.count() == 1, f"{engine} did not expose the semantic pointer target")
    before = int(dataset(page).get("data-kael-accessibility-action-count", "0"))
    button.click(force=True)
    page.wait_for_function(
        "count => Number(document.documentElement.dataset.kaelAccessibilityActionCount) > count",
        arg=before,
        timeout=10_000,
    )
    ime = page.locator('[data-kael-ime-input="true"]')
    require(ime.count() == 1, f"{engine} did not retain the IME bridge")
    ime.focus()
    page.keyboard.press("ArrowLeft")
    return {
        "pointer_action_count": int(
            dataset(page).get("data-kael-accessibility-action-count", "0")
        ),
        "ime": dataset(page).get("data-kael-ime-probe"),
        "clipboard": dataset(page).get("data-kael-clipboard-probe"),
    }


def verify_browser_smoke(
    browser: Browser,
    engine: str,
    base_url: str,
    output_dir: Path,
    viewport: dict[str, int],
    interactive: bool,
) -> dict[str, Any]:
    context = browser.new_context(viewport=viewport, accept_downloads=True)
    page = context.new_page()
    diagnostics = {
        "console": [],
        "page_errors": [],
        "request_failures": [],
        "beacons": [],
    }
    attach_diagnostics(page, diagnostics)
    label = f"{engine} browser smoke {viewport['width']}x{viewport['height']}"
    software_renderer = os.environ.get("KAEL_BROWSER_MATRIX_SOFTWARE") == "1"
    query = "?software_renderer=1" if software_renderer else ""
    page.goto(
        f"{base_url}/browser-smoke/{query}",
        wait_until="domcontentloaded",
        timeout=30_000,
    )
    wait_for_terminal_marker(
        page,
        """() => document.documentElement.dataset.kaelReady === "true" ||
          Boolean(document.querySelector("#failure")?.textContent?.trim())""",
        40_000,
    )
    failure = page_failure(page)
    require(not failure, f"{label} failed: {failure}")
    root = dataset(page)
    require(root.get("data-kael-ready") == "true", f"{label} never became ready")
    require(root.get("data-kael-virtual-table") == "verified", f"{label} table was not verified")
    require(root.get("data-kael-virtual-jump") == "last-row-visible", f"{label} O(1) jump failed")
    require(
        root.get("data-kael-virtual-scrollbar") == "always-visible",
        f"{label} did not retain the always-visible table scrollbar",
    )
    require(
        int(root.get("data-kael-virtual-logical-rows", "0")) == 1_000_000,
        f"{label} row count differed",
    )
    mounted = int(root.get("data-kael-virtual-mounted-rows", "0"))
    mount_bound = int(root.get("data-kael-virtual-mount-bound", "0"))
    require(1 < mounted <= mount_bound == 64, f"{label} mounted {mounted}/{mount_bound} rows")
    require(
        root.get("data-kael-virtual-performance") == "passed",
        f"{label} million-row latency gate failed",
    )
    expected_performance_class = (
        "software-fallback-liveness" if software_renderer else "hardware-performance"
    )
    require(
        root.get("data-kael-virtual-performance-class") == expected_performance_class,
        f"{label} used the wrong performance class",
    )
    require(root.get("data-kael-gpu-probe") == "reported", f"{label} did not report its GPU")
    gpu_is_software = root.get("data-kael-gpu-software-emulated") == "true"
    if not software_renderer:
        require(
            not gpu_is_software,
            f"{label} hardware gate used software renderer {root.get('data-kael-gpu-device')}",
        )
    performance_samples = int(root.get("data-kael-virtual-performance-samples", "0"))
    scroll_p50_ms = float(root.get("data-kael-virtual-scroll-p50-ms", "inf"))
    scroll_p95_ms = float(root.get("data-kael-virtual-scroll-p95-ms", "inf"))
    scroll_p99_ms = float(root.get("data-kael-virtual-scroll-p99-ms", "inf"))
    materialize_p99_us = float(
        root.get("data-kael-virtual-materialize-p99-us", "inf")
    )
    long_tasks = int(root.get("data-kael-virtual-long-tasks", "999999"))
    peak_mounted = int(root.get("data-kael-virtual-peak-mounted-rows", "999999"))
    require(performance_samples >= 16, f"{label} collected {performance_samples} latency samples")
    p95_budget = 1_000 if software_renderer else 80
    p99_budget = 2_000 if software_renderer else 160
    long_task_budget = 24 if software_renderer else 1
    require(
        0 <= scroll_p50_ms <= scroll_p95_ms <= scroll_p99_ms <= p99_budget,
        f"{label} scroll percentiles regressed: {scroll_p50_ms}/{scroll_p95_ms}/{scroll_p99_ms}ms",
    )
    require(scroll_p95_ms <= p95_budget, f"{label} scroll p95 was {scroll_p95_ms}ms")
    require(
        0 <= materialize_p99_us <= 20_000,
        f"{label} materialization p99 was {materialize_p99_us}us",
    )
    require(
        long_tasks <= long_task_budget,
        f"{label} observed {long_tasks} long tasks",
    )
    require(1 < peak_mounted <= 64, f"{label} peak-mounted {peak_mounted} rows")
    require(root.get("data-kael-webview-message") == "received", f"{label} iframe bridge failed")
    require(
        int(root.get("data-kael-webview-message-count", "0")) == 1,
        f"{label} iframe message count differed",
    )
    require(root.get("data-kael-text-probe") == "passed", f"{label} text shaping failed")
    require(root.get("data-kael-ime-probe") == "passed", f"{label} IME bridge failed")
    require(
        root.get("data-kael-clipboard-bounds") in {"passed", "automation-unavailable"},
        f"{label} clipboard intake bounds were not verified",
    )
    require(
        root.get("data-kael-accessibility-probe") == "passed",
        f"{label} accessibility mirror failed",
    )
    require(
        root.get("data-kael-accessibility-grid") == "passed",
        f"{label} virtual grid semantics failed",
    )
    canvases = canvas_evidence(page)
    assert_primary_canvas(page, canvases, label)
    primary = next(item for item in canvases if item["id"] == "blade")
    require(primary["contextRecovery"] == "verified", f"{label} context recovery failed")
    require(
        primary["contextRecoveryHash"] == primary["pixelHash"],
        f"{label} recovered framebuffer hash differed",
    )
    capabilities = capability_snapshot(page)
    assert_required_capabilities(capabilities, engine)
    result: dict[str, Any] = {
        "viewport": viewport,
        "dataset": root,
        "canvases": canvases,
        "capabilities": capabilities,
        "diagnostics": diagnostics,
    }
    if interactive:
        result["semantic_input"] = exercise_semantic_input(page, engine)
        result["files"] = exercise_file_apis(page, engine, output_dir)
    assert_clean_runtime_diagnostics(page, diagnostics, label)
    screenshot = output_dir / f"{engine}-browser-smoke-{viewport['width']}x{viewport['height']}.png"
    capture_retained_page_screenshot(page, screenshot)
    result["screenshot"] = screenshot.name
    result["framebuffer"] = f"{screenshot.stem}-framebuffer.png"
    context.close()
    assert_recorded_runtime_diagnostics(diagnostics, label)
    return result


def verify_suite_smoke(
    browser: Browser,
    engine: str,
    base_url: str,
    output_dir: Path,
    viewport: dict[str, int],
) -> dict[str, Any]:
    label = f"{engine} suite smoke {viewport['width']}x{viewport['height']}"
    context = browser.new_context(viewport=viewport)
    page = context.new_page()
    diagnostics = {
        "console": [],
        "page_errors": [],
        "request_failures": [],
        "beacons": [],
    }
    attach_diagnostics(page, diagnostics)
    page.goto(f"{base_url}/browser-suite-smoke/", wait_until="domcontentloaded", timeout=30_000)
    wait_for_terminal_marker(
        page,
        """() => ["true", "false"].includes(
          document.documentElement.dataset.kaelSuiteReady
        )""",
        70_000,
    )
    failure = page_failure(page)
    require(not failure, f"{label} failed: {failure}")
    root = dataset(page)
    require(root.get("data-kael-suite-ready") == "true", f"{label} never became ready")
    expected = {
        "data-kael-suite-workloads": "passed",
        "data-kael-suite-sheet-cache": "passed",
        "data-kael-suite-sheet-selection": "passed",
        "data-kael-suite-document-virtual": "passed",
        "data-kael-suite-slides-virtual": "passed",
        "data-kael-suite-whiteboard-render": "passed",
        "data-kael-suite-frame-export": "passed",
        "data-kael-suite-multi-window": "passed",
        "data-kael-suite-pointer": "passed",
        "data-kael-suite-route-lifecycle": "passed",
        "data-kael-suite-hash-route": "passed",
        "data-kael-suite-popstate-route": "passed",
        "data-kael-suite-no-reload": "passed",
        "data-kael-suite-bounded-mounts": "passed",
        "data-kael-suite-sheet-live-mounts": "passed",
    }
    differed = {name: root.get(name) for name, value in expected.items() if root.get(name) != value}
    require(not differed, f"{label} markers differed: {differed}")
    require(
        int(root.get("data-kael-suite-sheet-rows", "0")) == 1_000_000,
        f"{label} sheet row count differed",
    )
    require(
        int(root.get("data-kael-suite-sheet-columns", "0")) == 16_384,
        f"{label} sheet column count differed",
    )
    require(
        int(root.get("data-kael-suite-sheet-render-columns", "0")) == 16_384,
        f"{label} retained sheet did not expose the full logical column range",
    )
    require(
        root.get("data-kael-suite-sheet-selection-representation") == "anchor_focus"
        and int(root.get("data-kael-suite-sheet-selection-stored", "0")) == 2
        and int(root.get("data-kael-suite-sheet-selection-count", "0"))
        == 16_384_000_000,
        f"{label} full-sheet selection was not represented in constant space",
    )
    cached_tiles = int(root.get("data-kael-suite-sheet-cached-pages", "999999"))
    maximum_tiles = int(root.get("data-kael-suite-sheet-max-pages", "0"))
    require(
        0 < cached_tiles <= maximum_tiles <= 8,
        f"{label} sheet tile cache was not bounded: {cached_tiles}/{maximum_tiles}",
    )
    require(
        int(root.get("data-kael-suite-sheet-mounted-cells", "999999")) <= 2_048,
        f"{label} model table mount exceeded 2,048 cells",
    )
    require(
        0 < int(root.get("data-kael-suite-sheet-live-mounted-rows", "0")) <= 64,
        f"{label} live row mount was not bounded",
    )
    require(
        0 < int(root.get("data-kael-suite-sheet-live-mounted-columns", "0")) <= 16,
        f"{label} live column mount was not bounded",
    )
    require(
        0 < int(root.get("data-kael-suite-sheet-live-mounted-cells", "0")) <= 1_024,
        f"{label} live cell mount was not bounded",
    )
    require(
        int(root.get("data-kael-suite-document-mounted-pages", "999999")) <= 8,
        f"{label} document page mount exceeded 8",
    )
    require(
        int(root.get("data-kael-suite-slides-mounted", "999999")) <= 16,
        f"{label} slide mount exceeded 16",
    )
    require(
        int(root.get("data-kael-suite-whiteboard-rendered", "999999")) <= 512,
        f"{label} whiteboard culling exceeded 512 shapes",
    )
    require(
        int(root.get("data-kael-suite-frame-export-bytes", "0")) > 1_024,
        f"{label} PNG export was empty",
    )
    require(
        int(root.get("data-kael-window-count", "0")) == 1,
        f"{label} secondary window was not cleaned up",
    )
    if viewport["width"] < 900:
        require(
            root.get("data-kael-suite-responsive-mount") == "passed",
            f"{label} did not prove compact offscreen mount and restoration",
        )
    wait_for_beacon(
        page,
        diagnostics,
        SUITE_COMPACT_BEACON if viewport["width"] < 900 else SUITE_WIDE_BEACON,
        label,
    )
    assert_clean_runtime_diagnostics(page, diagnostics, label)
    canvases = canvas_evidence(page)
    assert_primary_canvas(page, canvases, label)
    screenshot = output_dir / f"{engine}-suite-smoke-{viewport['width']}x{viewport['height']}.png"
    page.screenshot(path=screenshot, full_page=True)
    result = {
        "dataset": root,
        "canvases": canvases,
        "diagnostics": diagnostics,
        "screenshot": screenshot.name,
        "viewport": viewport,
    }
    context.close()
    assert_recorded_runtime_diagnostics(diagnostics, label)
    return result


def verify_capture_smoke(
    browser: Browser,
    engine: str,
    base_url: str,
    output_dir: Path,
) -> dict[str, Any]:
    label = f"{engine} browser capture smoke"
    context = browser.new_context(viewport=DESKTOP_VIEWPORT)
    page = context.new_page()
    diagnostics = {
        "console": [],
        "page_errors": [],
        "request_failures": [],
        "beacons": [],
    }
    attach_diagnostics(page, diagnostics)
    fixture_capabilities = page.evaluate(
        """() => ({
          canvasCaptureStream:
            typeof HTMLCanvasElement.prototype.captureStream === "function",
          mediaStream: typeof MediaStream === "function",
        })"""
    )
    require(
        fixture_capabilities["canvasCaptureStream"]
        and fixture_capabilities["mediaStream"],
        f"{label} automation fixture is unavailable: {fixture_capabilities}; "
        "this does not imply that the trusted getDisplayMedia picker is unavailable",
    )
    page.goto(
        f"{base_url}/browser-capture-smoke/",
        wait_until="domcontentloaded",
        timeout=30_000,
    )
    wait_for_terminal_marker(
        page,
        """() => location.search.includes("__kael_capture_pass__=1") ||
          location.search.includes("__kael_capture_failed__=1")""",
        30_000,
    )
    query = {
        key: values[-1]
        for key, values in parse_qs(urlparse(page.url).query, keep_blank_values=True).items()
    }
    require(
        query.get("__kael_capture_pass__") == "1",
        f"{label} failed: {query}",
    )
    expected = {
        "enumeration": "passed",
        "start": "passed",
        "frames": "passed",
        "lifecycle": "passed",
        "bounds": "passed",
        "async_error": "passed",
    }
    differed = {
        name: query.get(name) for name, expected_value in expected.items()
        if query.get(name) != expected_value
    }
    require(not differed, f"{label} markers differed: {differed}")
    wait_for_beacon(page, diagnostics, CAPTURE_BEACON, label)
    assert_clean_runtime_diagnostics(page, diagnostics, label)
    readback_warnings = [
        message
        for message in diagnostics["console"]
        if "willReadFrequently" in message
    ]
    require(
        not readback_warnings,
        f"{label} did not request a readback-optimized 2D context: {readback_warnings}",
    )
    screenshot = output_dir / f"{engine}-capture-smoke.png"
    page.screenshot(path=screenshot, full_page=True)
    result = {
        "status": "passed",
        "query": query,
        "fixture_capabilities": fixture_capabilities,
        "frame_contract": {
            "width": 64,
            "height": 32,
            "format": "rgba32",
            "bytes": 64 * 32 * 4,
        },
        "viewport_independent_fixture": True,
        "trusted_picker": "not-automated-requires-user-activation",
        "diagnostics": diagnostics,
        "screenshot": screenshot.name,
    }
    context.close()
    assert_recorded_runtime_diagnostics(diagnostics, label)
    return result


def wait_for_echo_server(process: subprocess.Popen[str], log_path: Path) -> None:
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise VerificationFailure(
                "WebSocket echo server exited with "
                f"{process.returncode}: {log_path.read_text(errors='replace')}"
            )
        try:
            with socket.create_connection(("127.0.0.1", 8134), timeout=0.2):
                return
        except OSError:
            time.sleep(0.1)
    raise VerificationFailure("WebSocket echo server did not listen on 127.0.0.1:8134")


def stop_process(process: subprocess.Popen[str]) -> None:
    if process.poll() is not None:
        return
    process.send_signal(signal.SIGTERM)
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)


def verify_realtime(
    browser: Browser,
    engine: str,
    base_url: str,
    output_dir: Path,
    echo_server: Path,
) -> dict[str, Any]:
    log_path = output_dir / f"{engine}-websocket-server.log"
    with log_path.open("w", encoding="utf-8") as log:
        process = subprocess.Popen(
            [str(echo_server), "8134"],
            cwd=echo_server.parent,
            stdout=log,
            stderr=subprocess.STDOUT,
            text=True,
        )
        try:
            wait_for_echo_server(process, log_path)
            context = browser.new_context(viewport=DESKTOP_VIEWPORT)
            page = context.new_page()
            diagnostics = {
                "console": [],
                "page_errors": [],
                "request_failures": [],
                "beacons": [],
            }
            attach_diagnostics(page, diagnostics)
            page.goto(
                f"{base_url}/browser-websocket-smoke/",
                wait_until="domcontentloaded",
                timeout=30_000,
            )
            wait_for_terminal_marker(
                page,
                """() => ["passed", "failed"].includes(
                  document.documentElement.dataset.kaelWebsocketProbe
                )""",
                40_000,
            )
            root = dataset(page)
            require(
                root.get("data-kael-websocket-probe") == "passed",
                f"{engine} WebSocket probe failed: {root.get('data-kael-websocket-error-detail')}",
            )
            wait_for_beacon(page, diagnostics, WEBSOCKET_BEACON, f"{engine} WebSocket probe")
            expected_console_errors = ()
            if engine == "webkit" and root.get("data-kael-websocket-reconnect") == "passed":
                # The first `/reconnect` socket is deliberately dropped so Kael
                # can prove ordered error/close/reconnect delivery. WebKit logs
                # that expected transport loss as a page-console error even
                # though the API reports it normally and the retry succeeds.
                expected_console_errors = (
                    "WebSocket connection to 'ws://127.0.0.1:8134/reconnect' failed: "
                    "The operation couldn’t be completed. Socket is not connected",
                )
            assert_clean_runtime_diagnostics(
                page,
                diagnostics,
                f"{engine} WebSocket probe",
                expected_console_errors,
            )
            context.close()
            assert_recorded_runtime_diagnostics(
                diagnostics,
                f"{engine} WebSocket probe",
                expected_console_errors,
            )
            return {"dataset": root, "diagnostics": diagnostics, "server_log": log_path.name}
        finally:
            stop_process(process)


def launch_browser(browser_type: BrowserType, engine: str) -> Browser:
    software_renderer = os.environ.get("KAEL_BROWSER_MATRIX_SOFTWARE") == "1"
    force_headless = os.environ.get("KAEL_BROWSER_MATRIX_HEADLESS") == "1"
    kwargs: dict[str, Any] = {
        "headless": force_headless
        or not (sys.platform == "darwin" and not software_renderer),
        "timeout": 60_000,
    }
    if engine == "chromium":
        kwargs["args"] = ["--enable-webgl"]
        if software_renderer:
            kwargs["args"].extend(
                ["--enable-unsafe-swiftshader", "--use-angle=swiftshader"]
            )
    elif engine == "firefox":
        if os.environ.get("KAEL_BROWSER_MATRIX_FIREFOX_HEADED") == "1":
            kwargs["headless"] = False
        kwargs["firefox_user_prefs"] = {
            "webgl.disabled": False,
            # Firefox forbids software WebGL by default. Release CI has no GPU,
            # so opt into Mesa's bounded software implementation explicitly.
            "webgl.forbid-software": False,
            "webgl.force-enabled": True,
        }
    return browser_type.launch(**kwargs)


def capture_engine_failure(browser: Browser, engine: str, output_dir: Path) -> None:
    snapshots: list[dict[str, Any]] = []
    for context_index, context in enumerate(browser.contexts):
        for page_index, page in enumerate(context.pages):
            prefix = f"{engine}-failure-{context_index}-{page_index}"
            snapshot: dict[str, Any] = {"url": page.url}
            try:
                screenshot = output_dir / f"{prefix}.png"
                page.screenshot(path=screenshot, full_page=True, timeout=5_000)
                snapshot["screenshot"] = screenshot.name
            except Exception as error:
                snapshot["screenshot_error"] = str(error)
            try:
                snapshot["dataset"] = dataset(page)
                snapshot["canvases"] = canvas_evidence(page)
                snapshot["failure"] = page_failure(page)
            except Exception as error:
                snapshot["inspection_error"] = str(error)
            snapshots.append(snapshot)
    (output_dir / f"{engine}-failure.json").write_text(
        json.dumps(snapshots, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )


def verify_engine(
    playwright: Playwright,
    engine: str,
    args: argparse.Namespace,
) -> dict[str, Any]:
    browser_type = getattr(playwright, engine)
    browser = launch_browser(browser_type, engine)
    started = time.monotonic()
    try:
        result: dict[str, Any] = {
            "browser_version": browser.version,
            "browser_smoke": [],
        }
        result["browser_smoke"].append(
            verify_browser_smoke(
                browser,
                engine,
                args.base_url.rstrip("/"),
                args.artifacts,
                DESKTOP_VIEWPORT,
                True,
            )
        )
        result["browser_smoke"].append(
            verify_browser_smoke(
                browser,
                engine,
                args.base_url.rstrip("/"),
                args.artifacts,
                NARROW_VIEWPORT,
                False,
            )
        )
        if not args.skip_capture:
            result["capture_smoke"] = verify_capture_smoke(
                browser,
                engine,
                args.base_url.rstrip("/"),
                args.artifacts,
            )
        if not args.skip_suite:
            result["suite_smoke"] = [
                verify_suite_smoke(
                    browser,
                    engine,
                    args.base_url.rstrip("/"),
                    args.artifacts,
                    viewport,
                )
                for viewport in (DESKTOP_VIEWPORT, SUITE_COMPACT_VIEWPORT)
            ]
        if not args.skip_realtime:
            require(args.echo_server is not None, "--echo-server is required for realtime checks")
            result["realtime"] = verify_realtime(
                browser,
                engine,
                args.base_url.rstrip("/"),
                args.artifacts,
                args.echo_server.resolve(),
            )
        result["duration_seconds"] = round(time.monotonic() - started, 3)
        return result
    except Exception:
        capture_engine_failure(browser, engine, args.artifacts)
        raise
    finally:
        browser.close()


def write_report(path: Path, report: dict[str, Any]) -> None:
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    temporary.replace(path)


def main() -> int:
    args = parse_args()
    if not args.workspace.is_dir():
        raise SystemExit(f"workspace does not exist: {args.workspace}")
    engines = [engine.strip() for engine in args.engines.split(",") if engine.strip()]
    invalid = [engine for engine in engines if engine not in ENGINES]
    if invalid or not engines:
        raise SystemExit(f"invalid browser engines: {', '.join(invalid) or '(none)'}")
    args.artifacts.mkdir(parents=True, exist_ok=True)
    report: dict[str, Any] = {
        "schema": 1,
        "playwright": "1.62.0",
        "engines": {},
        "failures": {},
    }
    with sync_playwright() as playwright:
        for engine in engines:
            print(f"==> verifying {engine}", flush=True)
            try:
                report["engines"][engine] = verify_engine(playwright, engine, args)
                print(f"{engine}: passed", flush=True)
            except Exception as error:  # Keep running to expose every engine gap.
                report["failures"][engine] = f"{type(error).__name__}: {error}"
                print(f"{engine}: failed: {error}", file=sys.stderr, flush=True)
                write_report(args.artifacts / "report.json", report)
    write_report(args.artifacts / "report.json", report)
    if report["failures"]:
        print(json.dumps(report["failures"], indent=2, sort_keys=True), file=sys.stderr)
        return 1
    print(f"Cross-browser matrix passed; report: {args.artifacts / 'report.json'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
