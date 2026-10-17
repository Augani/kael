#!/usr/bin/env python3
"""Isolated hosted control; never substitutes for Kael's presentation gate."""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time


HERE = Path(__file__).resolve().parent
EVIDENCE = Path(os.environ.get("METAL_CONTROL_EVIDENCE", "target/metal-presentation-control")).resolve()
EVIDENCE.mkdir(parents=True, exist_ok=True)
RESULT: dict = {
    "contract": "independent-metal-presentation-control-v1",
    "scope": "Correctness/environment diagnostic only; not a latency/power benchmark or Kael runtime acceptance.",
    "recorded_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    "commands": [],
    "runs": [],
}


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def command(name: str, argv: list[str], timeout: int = 30) -> tuple[int, str]:
    try:
        process = subprocess.run(argv, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                 text=True, timeout=timeout, check=False)
        code, output = process.returncode, process.stdout
    except subprocess.TimeoutExpired as error:
        output = error.stdout or b""
        if isinstance(output, bytes):
            output = output.decode("utf-8", errors="replace")
        code = 124
        output += f"\nCONTROL_RUNNER_TIMEOUT seconds={timeout}\n"
    log = EVIDENCE / f"{name}.log"
    log.write_text(output)
    RESULT["commands"].append({"name": name, "argv": argv, "exit_code": code,
                               "log": log.name, "sha256": sha(log)})
    return code, output


def save() -> None:
    (EVIDENCE / "result.json").write_text(json.dumps(RESULT, indent=2) + "\n")


def main() -> int:
    if sys.platform != "darwin":
        raise SystemExit("This control requires a real macOS AppKit session.")
    for name, argv in [
        ("os", ["sw_vers"]),
        ("architecture", ["uname", "-m"]),
        ("compiler", ["xcrun", "clang", "--version"]),
        ("displays", ["system_profiler", "SPDisplaysDataType"]),
        ("virtualization", ["sysctl", "kern.hv_vmm_present"]),
        ("source-commit", ["git", "rev-parse", "HEAD"]),
    ]:
        command(name, argv)
    source, binary = HERE / "probe.m", EVIDENCE / "metal-presentation-control"
    RESULT["source_sha256"] = sha(source)
    compile_argv = ["xcrun", "clang", "-fobjc-arc", "-fblocks", "-Wall", "-Wextra", "-Werror",
                    "-framework", "AppKit", "-framework", "Metal", "-framework", "MetalKit",
                    "-framework", "QuartzCore", "-framework", "ApplicationServices",
                    str(source), "-o", str(binary)]
    code, output = command("compile", compile_argv, timeout=60)
    if code:
        print(output)
        save()
        return code
    RESULT["binary_sha256"] = sha(binary)
    command("pre-wake", [str(binary), "--diagnose"])
    wake_argv = ["/usr/bin/caffeinate", "-u", "-d", "-t", "45"]
    RESULT["wake_argv"] = wake_argv
    # This changes display power state only. It cannot unlock a session or
    # relax macOS permissions. The assertion expires even if this runner dies.
    assertion = subprocess.Popen(wake_argv, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    RESULT["assertion_pid"] = assertion.pid
    try:
        time.sleep(1)
        command("wake-assertions", ["pmset", "-g", "assertions"])
        command("post-wake", [str(binary), "--diagnose"])
        for name, args in [
            ("mtk", []),
            ("layer", ["--layer"]),
            ("layer-transaction", ["--layer", "--transaction"]),
        ]:
            code, output = command(name, [str(binary), *args], timeout=15)
            summary = next((line for line in output.splitlines()
                            if line.startswith("CONTROL_RESULT ")), None)
            fields = dict(token.split("=", 1) for token in summary.split()[1:]) if summary else {}
            passed = (code == 0 and fields.get("result") == "PASS"
                      and fields.get("submitted") == "60" and fields.get("completed") == "60"
                      and int(fields.get("presented", "0")) > 0
                      and fields.get("failures") == "0" and fields.get("geometry") == "1")
            RESULT["runs"].append({"name": name, "exit_code": code, "summary": summary,
                                   "fields": fields, "passed": passed})
            print(summary or f"CONTROL_RESULT_MISSING mode={name} exit={code}", flush=True)
        command("post-controls", [str(binary), "--diagnose"])
    finally:
        if assertion.poll() is None:
            assertion.terminate()
            try:
                assertion.wait(timeout=3)
            except subprocess.TimeoutExpired:
                assertion.kill()
                assertion.wait()
        RESULT["assertion_exit_code"] = assertion.returncode
        RESULT["all_controls_passed"] = (len(RESULT["runs"]) == 3
                                          and all(run["passed"] for run in RESULT["runs"]))
        save()
    return 0 if RESULT["all_controls_passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
