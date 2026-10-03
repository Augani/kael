#!/usr/bin/env python3
"""Alternate two real desktop binaries; preserve raw logs and process counters."""
import argparse
import hashlib
import json
import math
import os
import platform
import statistics
import subprocess
import time
from pathlib import Path
from workload_validation import TREE_HASHES, WORKSPACE_HASHES, validate_tree, validate_workspace

PHASE_NAMES = ('idle-before', 'active', 'idle-after', 'churn')
DATA_HASHES = {
    (0, False): 'fnv1a64:4ea6da623acce6b5',
    (0, True): 'fnv1a64:2c24f28356e1729d',
    (1, False): 'fnv1a64:cc17af116933398f',
    (1, True): 'fnv1a64:45df178322aed8a3',
}


def same_geometry(left, right):
    # Allow floating-point representation noise, but reject a one-pixel resize.
    return all(math.isclose(a, b, rel_tol=0, abs_tol=tolerance)
               for a, b, tolerance in zip(left, right, (0.01, 0.01, 0.000001)))


def validate_native_geometry(result):
    samples = result.get('native_window_geometry')
    if not isinstance(samples, list) or len(samples) != 8:
        raise RuntimeError('missing actual native phase-boundary geometry')
    expected = [(phase, boundary) for phase in PHASE_NAMES for boundary in ('begin', 'end')]
    first = None
    for sample, (phase, boundary) in zip(samples, expected):
        if not isinstance(sample, dict) or (sample.get('phase'), sample.get('boundary')) != (phase, boundary):
            raise RuntimeError('missing, duplicated or reordered native geometry boundaries')
        geometry = tuple(sample.get(key) for key in
                         ('viewport_width_px', 'viewport_height_px', 'scale_factor'))
        if any(type(value) not in (int, float) or not math.isfinite(value) or value <= 0
               for value in geometry):
            raise RuntimeError('invalid actual native geometry')
        if first is None:
            first = geometry
        elif not same_geometry(first, geometry):
            raise RuntimeError('native viewport or display scale changed during measurement')
    return first


def validate_paired_geometry(left, right):
    if {left.get('engine'), right.get('engine')} != {'kael', 'gpui-kit'}:
        raise RuntimeError('native geometry requires both measured engines')
    if not same_geometry(validate_native_geometry(left), validate_native_geometry(right)):
        raise RuntimeError('Kael and GPUI Kit actual native viewport or display scale differ')


def validate_phase_frames(result, frame_timing):
    phases = result.get('phase_frame_counts')
    if not isinstance(phases, list) or len(phases) != 4:
        raise RuntimeError('missing measured phase frame counts')
    for phase, name in zip(phases, PHASE_NAMES):
        if not isinstance(phase, dict) or phase.get('phase') != name:
            raise RuntimeError('missing, duplicated or reordered measured frame phases')
        if any(type(phase.get(key)) is not int or not 0 <= phase[key] < 2**64
               for key in ('renders', 'draws', 'submissions')):
            raise RuntimeError('invalid measured phase frame counts')
        if name in ('active', 'churn'):
            if phase['renders'] < 2:
                raise RuntimeError('active workload did not repaint after its initial phase frame')
            if frame_timing and (phase['draws'] < 2 or phase['submissions'] < 2):
                raise RuntimeError('missing native draw/submission activity during measured phase')
        if not frame_timing and (phase['draws'] or phase['submissions']):
            raise RuntimeError('native phase frame instrumentation unexpectedly active in disabled build')


def validate_workload(result, phases, engine, frame_timing, contract, rows):
    """Reject incomplete or altered workloads before deriving any comparison."""
    if result.get('contract') != contract or result.get('rows') != rows or result.get('engine') != engine:
        raise RuntimeError('workload contract mismatch')
    if result.get('quick', False):
        raise RuntimeError('quick smoke workloads cannot satisfy measured comparison')
    validate_native_geometry(result)
    validate_phase_frames(result, frame_timing)
    component_contract = contract in ('native-editor-document-v1', 'native-data-table-v1',
                                      'native-virtual-tree-v1', 'native-dock-workspace-v1')
    names = [phase.get('phase') for phase in phases]
    expected_names = ([name for phase in PHASE_NAMES for name in (phase, 'validation')]
                      + ['finished']) if component_contract else list(PHASE_NAMES)
    if names != expected_names:
        raise RuntimeError('missing, duplicated or reordered phase markers')
    elapsed = [phase.get('elapsed_us') for phase in phases] + [result.get('elapsed_us')]
    if any(type(value) is not int or value < 0 for value in elapsed):
        raise RuntimeError('invalid phase timestamps')
    # Permit 100 ms of reporting tolerance, without accepting shortened smoke
    # workloads. Boundary oracle checks may make a phase longer, never shorter.
    if component_contract:
        if result.get('validation_phase_markers') is not True:
            raise RuntimeError('oracle work must be excluded from measured process phases')
        measured_intervals = [(elapsed[index], elapsed[index + 1]) for index in (0, 2, 4, 6)]
    else:
        measured_intervals = list(zip(elapsed, elapsed[1:]))
    if any(end < start for start, end in zip(elapsed, elapsed[1:])) or any(
            end - start < minimum for (start, end), minimum in
            zip(measured_intervals, (4_900_000, 11_900_000, 4_900_000, 11_900_000))):
        raise RuntimeError('shortened or non-monotonic workload phases')
    expected_fixture = {}
    operations = ()
    if contract == 'native-editor-document-v1':
        expected_fixture = {
            'sections': 2_000, 'fixture_hash': 'fnv1a64:694401c508afd37d',
            'fixture_bytes': 416_000, 'font_family': 'Menlo', 'font_size_px': 14.0,
            'line_height_px': 21.0, 'window_width_px': 1_100,
            'window_height_px': 760, 'syntax': 'plain',
            'theme_mode': 'dark',
        }
        operations = ('selection', 'replace', 'undo', 'redo', 'scroll_to_caret',
                      'home', 'replace_document')
    elif contract == 'native-data-table-v1':
        expected_fixture = {
            'columns': ['Title', 'Owner', 'Status', 'Score', 'Sprint', 'Updated', 'Tags', 'Notes'],
            'column_count': 8, 'fixture_hash': 'fnv1a64:4ea6da623acce6b5',
            'fixture_bytes': 12_166_666, 'fixture_retained_datasets': 2,
            'font_family': 'Menlo', 'font_size_px': 13.0, 'header_font_size_px': 12.0,
            'line_height_px': 20.0, 'row_height_px': 28.0, 'column_width_px': 112.0,
            'window_width_px': 1_100, 'window_height_px': 760,
            'table_width_px': 780.0, 'table_height_px': 704,
            'fixed_columns': 1, 'header_height_px': 32,
            'leaf_header_height_px': 32 if engine == 'kael' else 28,
            'theme_mode': 'dark', 'native_cell_editing': False, 'cell_wrap': 'nowrap',
        }
        operations = ('selection_reveal', 'vertical_scroll', 'horizontal_scroll',
                      'query_reverse', 'home', 'end_selection', 'replace_model')
        oracles = result.get('phase_oracles', [])
        if not isinstance(oracles, list) or len(oracles) != 4 or any(
                not isinstance(oracle, dict)
                or not isinstance(oracle.get('selected_cell'), list)
                or len(oracle['selected_cell']) != 2
                or any(type(value) is not int or not 0 <= value < limit
                       for value, limit in zip(oracle['selected_cell'], (100_000, 8)))
                or type(oracle.get('verified_control_row')) is not int
                or not 0 <= oracle['verified_control_row'] < 100_000
                or oracle.get('phase') != phase or oracle.get('correct') is not True
                or oracle.get('verified_model_cells') != 800_000
                or oracle.get('verified_control_cells') != 8
                or type(oracle.get('dataset_generation')) is not int
                or oracle['dataset_generation'] not in (0, 1)
                or type(oracle.get('query_reversed')) is not bool
                or oracle.get('ordered_hash') != DATA_HASHES[(oracle['dataset_generation'], oracle['query_reversed'])]
                or type(oracle.get('native_viewport_rows')) is not int
                or not 1 <= oracle['native_viewport_rows'] <= 64
                or type(oracle.get('native_viewport_scrollable_columns')) is not int
                or not 1 <= oracle['native_viewport_scrollable_columns'] <= 8
                for oracle, phase in zip(oracles, PHASE_NAMES)):
            raise RuntimeError('missing native data-control oracles or bounded viewport proof')
        if result.get('final_hash') != oracles[-1]['ordered_hash']:
            raise RuntimeError('final data query differs from the last verified control snapshot')
    elif contract == 'native-virtual-tree-v1':
        expected_fixture = {
            'root_count': 25, 'children_per_root': 4000, 'fixture_hash': TREE_HASHES[0],
            'fixture_bytes': 6_100_975, 'fixture_hashes': TREE_HASHES,
            'fixture_retained_datasets': 2, 'replacement_interval_updates': 30,
            'tree_width_px': 780.0, 'tree_height_px': 704.0, 'row_height_px': 28.0,
            'indent_px': 14.0, 'horizontal_scroll': False,
            'font_family': 'Menlo', 'font_size_px': 14.0, 'line_height_px': 20.0,
            'window_width_px': 1100, 'window_height_px': 760, 'theme_mode': 'dark',
            'component': 'VirtualTreeList' if engine == 'kael' else 'Tree',
        }
        operations = ('selection_reveal', 'vertical_scroll', 'collapse_root', 'expand_root',
                      'home', 'end_selection', 'replace_model')
        validate_tree(result)
    elif contract == 'native-dock-workspace-v1':
        expected_fixture = {
            'pane_count': 12, 'body_lines_per_pane': 32, 'fixture_hash': WORKSPACE_HASHES[0],
            'fixture_bytes': 22_560, 'fixture_hashes': WORKSPACE_HASHES,
            'fixture_retained_datasets': 2, 'replacement_interval_updates': 32,
            'workspace_width_px': 1100.0, 'workspace_height_px': 704.0,
            'initial_tab_groups': 3, 'initial_split_count': 2,
            'initial_split_axes': ['horizontal', 'vertical'], 'initial_split_ratios': [0.5, 0.5],
            'native_chrome_geometry': True, 'floating_panes': False, 'edge_docks': False,
            'font_family': 'Menlo', 'font_size_px': 14.0, 'line_height_px': 20.0,
            'window_width_px': 1100, 'window_height_px': 760, 'theme_mode': 'dark',
            'component': 'DockWorkspace' if engine == 'kael' else 'DockArea+DockSkin',
        }
        operations = ('select_tab', 'move_tab', 'split_pane', 'merge_pane', 'resize_split',
                      'zoom_group', 'unzoom_group', 'serialize_restore', 'replace_model')
        validate_workspace(result)
    if component_contract:
        if any(result.get(key) != value
               or (type(value) is not bool and type(result.get(key)) is bool)
               for key, value in expected_fixture.items()):
            raise RuntimeError('component fixture, typography or window contract mismatch')
        if result.get('phase_correctness') != [[name, True] for name in PHASE_NAMES]:
            raise RuntimeError('missing exact-byte component phase checks')
        if any(type(result.get('operations', {}).get(key)) is not int
               or result['operations'][key] <= 0
               for key in operations):
            raise RuntimeError('component did not execute every required operation')
    reported_mode = result.get('frame_timing_enabled', True)
    if type(reported_mode) is not bool or reported_mode != frame_timing:
        raise RuntimeError('framework frame timing mode does not match requested measurement')
    for key in ('draw_cpu_us', 'submission_cpu_us'):
        values = result.get(key)
        if not isinstance(values, list) or len(values) > 4096 or any(
                type(value) not in (int, float) or not math.isfinite(value) or value < 0
                for value in values):
            raise RuntimeError(f'invalid or unbounded {key} records')
    if frame_timing:
        first = result.get('first_submission_us')
        if (not result['draw_cpu_us'] or not result['submission_cpu_us']
                or type(first) is not int or first < 0 or first > result['elapsed_us']):
            raise RuntimeError('missing frame/submission instrumentation')
    elif result['draw_cpu_us'] or result['submission_cpu_us'] or result.get('first_submission_us') is not None:
        raise RuntimeError('framework timing unexpectedly active in disabled build')


def command(*args):
    return subprocess.check_output(args, text=True).strip()


def digest(path):
    with path.open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def optional_command(*args):
    completed = subprocess.run(args, capture_output=True, text=True, timeout=30)
    return {'exit_code': completed.returncode, 'stdout': completed.stdout.strip(),
            'stderr': completed.stderr.strip()}


def source_fingerprint():
    # Include new files as well as tracked changes. Binary digests independently
    # identify the executables; this describes the checkout during measurement.
    names = subprocess.check_output(['git', 'ls-files', '-z', '--cached', '--others', '--exclude-standard'])
    files = {}
    for name in sorted(set(os.fsdecode(name) for name in names.split(b'\0') if name)):
        path = Path(name)
        if path.is_file() and (name.startswith(('crates/', 'vendor/', 'benchmarks/desktop-comparison/'))
                               or name in ('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml')):
            files[name] = digest(path)
    encoded = json.dumps(files, sort_keys=True).encode()
    return {'sha256': hashlib.sha256(encoded).hexdigest(), 'files': files}


def quantile(samples, fraction):
    if not samples:
        return None
    ordered = sorted(samples)
    return ordered[min(len(ordered) - 1, int((len(ordered) - 1) * fraction))]


def run(engine, executable, helper, destination, index, frame_timing=True,
        contract='native-navigation-detail-v1', rows=100_000, activation_helper=None):
    log = destination / f'{index:02}-{engine}.log'
    samples = []
    phases = []
    result = None
    pending = ''
    phase = 'startup'
    window_activations = []
    with log.open('w') as output, log.open() as stream:
        started = time.monotonic()
        child = subprocess.Popen([str(executable.resolve())], stdout=output, stderr=subprocess.STDOUT)
        try:
            while child.poll() is None:
                if time.monotonic() - started > 75:
                    raise TimeoutError(f'{engine} failed to complete its workload within 75 seconds')
                try:
                    counters = json.loads(command(str(helper), str(child.pid)))
                    samples.append({'elapsed_s': time.monotonic() - started, 'phase': phase, **counters})
                except (subprocess.CalledProcessError, json.JSONDecodeError):
                    pass
                pending += stream.read()
                while '\n' in pending:
                    line, pending = pending.split('\n', 1)
                    if line.startswith('KAEL_PHASE '):
                        marker = json.loads(line.removeprefix('KAEL_PHASE '))
                        phase = marker['phase']
                        phases.append(marker)
                        if activation_helper is not None and phase in ('active', 'churn'):
                            accepted = json.loads(command(str(activation_helper), str(child.pid)))
                            if accepted.get('activation_accepted') is not True:
                                raise RuntimeError('owned native window activation was not accepted')
                            window_activations.append({'phase': phase, **accepted})
                time.sleep(.25)
            return_code = child.wait()
        finally:
            if child.poll() is None:
                child.terminate()
                try:
                    child.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait()
    # Re-read the complete capture so a final marker written just before process
    # exit cannot disappear between external sampling polls.
    phases = []
    for line in log.read_text().splitlines():
        if line.startswith('KAEL_PHASE '):
            phases.append(json.loads(line.removeprefix('KAEL_PHASE ')))
        if line.startswith('KAEL_COMPARISON '):
            if result is not None:
                raise RuntimeError(f'{log}: more than one result')
            result = json.loads(line.removeprefix('KAEL_COMPARISON '))
    if return_code != 0 or result is None:
        raise RuntimeError(f'{engine} returned {return_code} without a completed workload; inspect {log}')
    validate_workload(result, phases, engine, frame_timing, contract, rows)
    if activation_helper is not None and [entry['phase'] for entry in window_activations] != ['active', 'churn']:
        raise RuntimeError('missing owned foreground phase activation')
    if len(samples) < 60:
        raise RuntimeError('missing phase markers or process samples')
    phase_metrics = {}
    for name in PHASE_NAMES:
        # Discard one boundary sample and avoid the transition repaint.
        selected = [sample for sample in samples if sample['phase'] == name][1:-1]
        if len(selected) < 2:
            raise RuntimeError(f'no stable samples for {name}')
        first, last = selected[0], selected[-1]
        elapsed = last['elapsed_s'] - first['elapsed_s']
        phase_metrics[name] = {
            'rss_median_bytes': statistics.median(sample['rss_bytes'] for sample in selected),
            'footprint_max_bytes': max(sample['footprint_bytes'] for sample in selected),
            'cpu_percent': ((last['user_ns'] + last['system_ns']) - (first['user_ns'] + first['system_ns'])) / elapsed / 1e7,
            'idle_wakeups_per_s': (last['idle_wakeups'] - first['idle_wakeups']) / elapsed,
            'interrupt_wakeups_per_s': (last['interrupt_wakeups'] - first['interrupt_wakeups']) / elapsed,
            'billed_energy_raw_delta': last['billed_energy_raw'] - first['billed_energy_raw'],
            'serviced_energy_raw_delta': last['serviced_energy_raw'] - first['serviced_energy_raw'],
        }
    report = {'engine': engine, 'index': index, 'binary_sha256': digest(executable),
              'process_samples': samples, 'phases': phases, 'application': result,
              'phase_window_activation': window_activations,
              'phase_metrics': phase_metrics,
              'draw_us': {str(q): quantile(result['draw_cpu_us'], q) for q in (.5, .95, .99)},
              'submission_us': {str(q): quantile(result['submission_cpu_us'], q) for q in (.5, .95, .99)}}
    (destination / f'{index:02}-{engine}.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({'completed': engine, 'run': index, 'first_submission_us': result['first_submission_us'], 'draw_us': report['draw_us'], 'phase_metrics': phase_metrics}), flush=True)
    return report


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--kael', type=Path, required=True)
    parser.add_argument('--gpui-kit', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--repetitions', type=int, default=5)
    parser.add_argument('--without-frame-timing', action='store_true',
                        help='require builds with optional framework frame instrumentation disabled')
    parser.add_argument('--contract', choices=('native-navigation-detail-v1',
                                              'native-editor-document-v1',
                                              'native-data-table-v1',
                                              'native-virtual-tree-v1',
                                              'native-dock-workspace-v1'),
                        default='native-navigation-detail-v1')
    args = parser.parse_args()
    if platform.system() != 'Darwin':
        parser.error('this process-counter adapter requires macOS; other OS adapters remain separate')
    if not 1 <= args.repetitions <= 20:
        parser.error('repetitions must be in 1..=20')
    destination = args.output.resolve()
    destination.mkdir(parents=True, exist_ok=True)
    helper = destination / 'process-metrics'
    subprocess.run(['cc', '-O2', str(Path(__file__).with_name('process_metrics_macos.c')), '-o', str(helper)], check=True)
    activation_helper = destination / 'activate-native-window'
    activation_source = Path(__file__).with_name('activate_owned_window_macos.m')
    subprocess.run(['cc', '-O2', str(activation_source), '-framework', 'AppKit',
                    '-o', str(activation_helper)], check=True)
    adapter_sample = json.loads(command(str(helper), str(os.getpid())))
    metadata = {'schema_version': 3, 'platform': platform.platform(), 'cpu': command('sysctl', '-n', 'machdep.cpu.brand_string'),
                'memory_bytes': int(command('sysctl', '-n', 'hw.memsize')), 'cpu_count': os.cpu_count(),
                'rustc': command('rustc', '-Vv'), 'load_average': os.getloadavg(),
                'power_source': command('pmset', '-g', 'batt'),
                'displays': optional_command('system_profiler', 'SPDisplaysDataType', '-json'),
                'thermal_conditions': optional_command('pmset', '-g', 'therm'),
                'framework_head': command('git', 'rev-parse', 'HEAD'),
                'framework_diff_sha256': hashlib.sha256(subprocess.check_output(['git', 'diff', '--binary'])).hexdigest(),
                'checkout_source_fingerprint': source_fingerprint(),
                'comparison_revision': '3467e647600290343885b500bd7464057e334d18',
                'command': vars(args) | {'kael': str(args.kael), 'gpui_kit': str(args.gpui_kit), 'output': str(args.output)},
                'counter_scope': 'libproc RUSAGE_INFO_V4 per-process; energy fields are raw OS counters, not watts',
                'cpu_timebase': {'numer': adapter_sample['timebase_numer'], 'denom': adapter_sample['timebase_denom'],
                                 'raw_units': 'Mach absolute ticks', 'converted_units': 'nanoseconds'},
                'adapter_source_sha256': digest(Path(__file__).with_name('process_metrics_macos.c')),
                'adapter_binary_sha256': digest(helper),
                'framework_frame_timing_enabled': not args.without_frame_timing,
                'window_activation_scope': 'request foreground for the owned child at active/churn begin; repaint activity remains mandatory',
                'activation_source_sha256': digest(activation_source),
                'activation_binary_sha256': digest(activation_helper),
                'native_geometry_scope': 'actual client viewport and scale at each phase begin/end; stable and equal across all captures required',
                'timing_scope': 'CPU draw and platform submission when enabled; common application callback timing always enabled; no GPU/compositor completion measurement'}
    (destination / 'environment.json').write_text(json.dumps(metadata, indent=2) + '\n')
    reports = []
    comparison_geometry = None
    for index in range(args.repetitions):
        order = [('kael', args.kael), ('gpui-kit', args.gpui_kit)]
        if index % 2:
            order.reverse()
        pair = []
        for engine, executable in order:
            report = run(engine, executable, helper, destination, index,
                         frame_timing=not args.without_frame_timing,
                         activation_helper=activation_helper,
                         contract=args.contract,
                         rows={'native-editor-document-v1': 16_001,
                               'native-virtual-tree-v1': 100_025,
                               'native-dock-workspace-v1': 12}.get(args.contract, 100_000))
            geometry = validate_native_geometry(report['application'])
            if comparison_geometry is None:
                comparison_geometry = geometry
            elif not same_geometry(comparison_geometry, geometry):
                raise RuntimeError('native viewport or display scale changed between captures')
            pair.append(report)
        validate_paired_geometry(pair[0]['application'], pair[1]['application'])
        reports.extend(pair)
    (destination / 'runs.json').write_text(json.dumps(reports, indent=2) + '\n')


if __name__ == '__main__':
    main()
