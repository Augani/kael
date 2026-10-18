"""Independent fixture and native-control validation for tree/dock captures."""
from functools import lru_cache
import math
import re

PHASES = ('idle-before', 'active', 'idle-after', 'churn')
TREE_HASHES = ['fnv1a64:0fbc5b81edc32f9b', 'fnv1a64:1bfc4b56d6a0a5eb']
WORKSPACE_HASHES = ['fnv1a64:8575d5bda29bda91', 'fnv1a64:42c3940aeb810ba1']
PANE_IDS = [f'pane-{index:02}' for index in range(12)]


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def integer(value, minimum, maximum):
    return type(value) is int and minimum <= value <= maximum


def oracles_for(result):
    oracles = result.get('phase_oracles')
    require(isinstance(oracles, list) and len(oracles) == 4,
            'missing native-control phase oracles')
    for phase, oracle in zip(PHASES, oracles):
        require(isinstance(oracle, dict) and oracle.get('phase') == phase
                and oracle.get('correct') is True, 'failed or reordered native-control oracle')
    return oracles


@lru_cache(maxsize=52)
def tree_hash(generation, collapsed):
    # Reconstruct the complete Unicode fixture independently of either engine.
    # Only the 52 resulting integers/strings are retained, never the documents.
    value = 0xcbf29ce484222325

    def add(field):
        nonlocal value
        encoded = field.encode('utf-8')
        for byte in len(encoded).to_bytes(8, 'little') + encoded:
            value = ((value ^ byte) * 0x100000001b3) & 0xffffffffffffffff

    for root in range(25):
        for field in (f'project/{root:02}', f'Project {root:02} · 日本語 café', '0'):
            add(field)
        if root != collapsed:
            for child in range(4000):
                for field in (f'project/{root:02}/file/{child:04}',
                              f'File {child:04} · 👩‍💻 naïve · rev {generation:02}', '1'):
                    add(field)
    return f'fnv1a64:{value:016x}'


def tree_index(node_id, collapsed):
    require(isinstance(node_id, str), 'missing native tree node identity')
    match = re.fullmatch(r'project/(\d{2})(?:/file/(\d{4}))?', node_id)
    require(match is not None, 'malformed native tree node identity')
    root = int(match[1])
    child = None if match[2] is None else int(match[2])
    require(root < 25 and (child is None or child < 4000)
            and not (root == collapsed and child is not None),
            'native tree identity is outside the visible projection')
    return root * 4001 + (0 if child is None else child + 1) - (
        4000 if collapsed is not None and root > collapsed else 0)


def validate_tree(result):
    for oracle in oracles_for(result):
        generation = oracle.get('dataset_generation')
        collapsed = oracle.get('collapsed_root')
        require(integer(generation, 0, 1) and
                (collapsed is None or integer(collapsed, 0, 24)),
                'invalid native tree generation or collapsed root')
        count = 100025 if collapsed is None else 96025
        require(type(oracle.get('visible_nodes')) is int and oracle['visible_nodes'] == count
                and type(oracle.get('verified_control_nodes')) is int
                and oracle['verified_control_nodes'] == count
                and oracle.get('fixture_hash') == TREE_HASHES[generation]
                and oracle.get('visible_hash') == tree_hash(generation, collapsed),
                'native tree projection contains stale, missing or altered nodes')
        mounted = oracle.get('mounted_indices')
        require(isinstance(mounted, list) and 1 <= len(mounted) <= 64
                and all(integer(index, 0, count - 1) for index in mounted)
                and mounted == sorted(set(mounted))
                and type(oracle.get('native_mounted_rows')) is int
                and oracle['native_mounted_rows'] == len(mounted)
                and oracle.get('reveal_target_mounted') is True,
                'missing bounded physical tree mounting proof')
        tree_index(oracle.get('selected_id'), collapsed)
        require(tree_index(oracle.get('scroll_target_id'), collapsed) in mounted,
                'native tree reveal target was not physically mounted')


def layout_panes(layout, depth=0):
    require(depth <= 32 and isinstance(layout, dict) and len(layout) == 1,
            'invalid native dock layout')
    if 'Tabs' in layout:
        tabs = layout['Tabs']
        require(isinstance(tabs, dict) and set(tabs) == {'panes', 'active'},
                'invalid native dock tab group')
        panes, active = tabs['panes'], tabs['active']
        require(isinstance(panes, list) and 1 <= len(panes) <= 12
                and all(pane in PANE_IDS for pane in panes) and active in panes,
                'missing or foreign dock pane')
        return panes, [active], active if 'pane-00' in panes else None
    require('Split' in layout, 'unknown native dock layout node')
    split = layout['Split']
    require(isinstance(split, dict) and set(split) == {'axis', 'children'}
            and split['axis'] in ('horizontal', 'vertical')
            and isinstance(split['children'], list) and 2 <= len(split['children']) <= 12,
            'invalid native dock split')
    panes, active, zoom_target = [], [], None
    for child in split['children']:
        child_panes, child_active, child_zoom = layout_panes(child, depth + 1)
        panes.extend(child_panes)
        active.extend(child_active)
        if child_zoom is not None:
            zoom_target = child_zoom
        require(len(panes) <= 12, 'unbounded or duplicate native dock panes')
    return panes, active, zoom_target


def validate_workspace(result):
    previous_roundtrips = 0
    for oracle in oracles_for(result):
        generation = oracle.get('fixture_generation')
        require(integer(generation, 0, 1) and
                oracle.get('fixture_hash') == WORKSPACE_HASHES[generation],
                'stale or altered native dock content')
        panes, active, zoom_target = layout_panes(oracle.get('actual_layout'))
        zoomed = oracle.get('native_zoomed')
        require(type(zoomed) is bool and sorted(panes) == PANE_IDS
                and oracle.get('verified_pane_ids') == PANE_IDS
                and oracle.get('verified_active_content') == ([zoom_target] if zoomed else active)
                and oracle.get('content_verified') is True,
                'native dock layout/content verification is incomplete')
        ratio = oracle.get('root_split_ratio')
        roundtrips = oracle.get('persistence_roundtrips_verified')
        require(type(ratio) in (int, float) and math.isfinite(ratio) and 0.05 <= ratio <= 0.95
                and integer(roundtrips, previous_roundtrips, 4096)
                and integer(oracle.get('native_layout_json_bytes'), 1, 1024 * 1024),
                'invalid native dock resize or persistence evidence')
        previous_roundtrips = roundtrips
    require(previous_roundtrips > 0, 'native dock persistence was never verified')
