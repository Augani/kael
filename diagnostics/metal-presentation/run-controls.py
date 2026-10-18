#!/usr/bin/env python3
"""Same-job independent diagnostic; never substitutes for Kael's positive gate."""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time

HERE = Path(__file__).resolve().parent
SOURCE_SHA256 = 'e61912654259c3a3495181401a0290c20053aaa93bd2dd121bdfc29010413fba'


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def parse_control(code: int, output: str) -> tuple[dict, bool]:
    summaries = [line for line in output.splitlines() if line.startswith('CONTROL_RESULT ')]
    if len(summaries) != 1:
        return {'summary_count': len(summaries)}, False
    try:
        fields = dict(token.split('=', 1) for token in summaries[0].split()[1:])
        counts = {key: int(fields[key]) for key in
                  ('submitted', 'completed', 'callbacks', 'presented', 'dropped', 'failures')}
    except (ValueError, KeyError):
        return {'summary': summaries[0], 'malformed': True}, False
    passed = (code == 0 and fields.get('result') == 'PASS'
              and counts['submitted'] == counts['completed'] == 60
              and 0 < counts['presented'] <= counts['callbacks'] <= 60
              and counts['dropped'] >= 0
              and counts['presented'] + counts['dropped'] == counts['callbacks']
              and counts['failures'] == 0 and fields.get('geometry') == '1')
    return {'summary': summaries[0], 'fields': fields}, passed


def command(evidence: Path, result: dict, name: str, argv: list[str], timeout: int) -> tuple[int, str]:
    """Bound only our child process group; record every failed/timeout command."""
    started = time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())
    process = None
    timed_out = False
    try:
        process = subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                   start_new_session=True)
        try:
            output, _ = process.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            timed_out = True
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                output, _ = process.communicate(timeout=3)
            except subprocess.TimeoutExpired:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                output, _ = process.communicate()
        code = 124 if timed_out else process.returncode
    except OSError as error:
        code, output = 127, (str(error) + '\n').encode()
    text = output.decode('utf-8', errors='replace')
    log = evidence / f'{name}.log'
    log.write_text(text)
    result['commands'].append({'name': name, 'argv': argv,
                               'owned_pid': process.pid if process else None,
                               'started_at_utc': started, 'exit_code': code,
                               'timed_out': timed_out, 'timeout_seconds': timeout,
                               'log': log.name, 'sha256': sha(log)})
    (evidence / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    return code, text


def main() -> int:
    if sys.platform != 'darwin':
        raise SystemExit('Independent AppKit controls require macOS; no acceptance skip.')
    evidence = Path(os.environ.get('METAL_CONTROL_EVIDENCE',
                    'target/native-renderer-smoke/macos/presentation-controls')).resolve()
    evidence.mkdir(parents=True, exist_ok=False)
    result = {'contract': 'same-job-independent-metal-presentation-control-v2',
              'recorded_at_utc': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
              'scope': 'Environment/functional diagnostic only; separate required Kael positive-presentation gate stays unchanged.',
              'source_sha256': sha(HERE / 'probe.m'), 'commands': [], 'runs': [],
              'all_controls_passed': False,
              'limits': {'frames_per_variant': 60, 'variant_timeout_seconds': 15,
                         'compile_timeout_seconds': 60, 'retries': 0},
              'data_scope': 'Selected console/login/lock-known fields, display geometry/power state, native control device name/registryID/unified-memory; no user names, raw session dictionary, environment or registry dumps.',
              'no_wake_unlock_or_permission_changes': True,
              'qualifications': ['Missing optional lock key means UNKNOWN, never unlocked.',
                                 'AppKit visible/active and GPU completion do not prove presentedTime>0.',
                                 'The independent probe selects MTLCreateSystemDefaultDevice; this does not identify Kael-selected device on a multi-GPU host.',
                                 'A control pass cannot turn a failed Kael gate into success. Zero controls require runner/session/provider investigation, not fabricated timestamps or skip.',
                                 'No latency, power or framework performance comparison.']}
    save = lambda: (evidence / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    save()
    if result['source_sha256'] != SOURCE_SHA256:
        result['fatal'] = 'Pinned independent control source changed'
        save()
        return 2
    (evidence / 'probe-source.m').write_bytes((HERE / 'probe.m').read_bytes())
    for name, argv in [
        ('os', ['sw_vers']),
        ('architecture', ['uname', '-m']),
        ('compiler', ['xcrun', 'clang', '--version']),
        ('hypervisor', ['sysctl', 'kern.hv_vmm_present']),
        ('checkout', ['git', 'rev-parse', 'HEAD']),
    ]:
        command(evidence, result, name, argv, 10)
    binary = evidence / 'metal-presentation-control'
    argv = ['xcrun', 'clang', '-fobjc-arc', '-fblocks', '-Wall', '-Wextra', '-Werror',
            '-framework', 'AppKit', '-framework', 'Metal', '-framework', 'MetalKit',
            '-framework', 'QuartzCore', '-framework', 'ApplicationServices',
            str(HERE / 'probe.m'), '-o', str(binary)]
    code, _ = command(evidence, result, 'compile', argv, 60)
    if code:
        result['fatal'] = 'Independent probe compile failed; original Kael gate is not reclassified'
        save()
        return code
    result['binary_sha256'] = sha(binary)
    command(evidence, result, 'session-display-before-controls', [str(binary), '--diagnose'], 10)
    for name, options in [('mtk', []), ('layer', ['--layer']),
                          ('layer-transaction', ['--layer', '--transaction'])]:
        code, output = command(evidence, result, name, [str(binary), *options], 15)
        parsed, passed = parse_control(code, output)
        result['runs'].append({'name': name, 'exit_code': code, 'passed': passed, **parsed})
        save()
        print(f'INDEPENDENT_METAL_CONTROL name={name} exit={code} passed={passed}', flush=True)
    command(evidence, result, 'session-display-after-controls', [str(binary), '--diagnose'], 10)
    result['all_controls_passed'] = len(result['runs']) == 3 and all(run['passed'] for run in result['runs'])
    save()
    return 0 if result['all_controls_passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
