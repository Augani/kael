#!/usr/bin/env python3
"""External AT-SPI client for an owned Kael virtual-tree test process."""
import argparse
from pathlib import Path
import time
import gi

gi.require_version('Atspi', '2.0')
from gi.repository import Atspi


def wait(read, seconds=30):
    deadline = time.monotonic() + seconds
    while True:
        try:
            return read()
        except Exception:
            if time.monotonic() >= deadline:
                raise
            time.sleep(.1)


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def children(node):
    node.clear_cache()
    count = node.get_child_count()
    require(0 <= count <= 4_000, 'native hierarchy exceeded fixture bounds')
    return count


def find_tree(app):
    # Search only the owned application's small top-level layout. Descendants
    # of the logical tree are explored separately, never scanned on the desktop.
    queue = [app]
    visited = 0
    while queue:
        node = queue.pop(0)
        visited += 1
        require(visited <= 512, 'native tree root not found within layout bound')
        if node.get_name() == 'Project files':
            return node
        queue.extend(node.get_child_at_index(index) for index in range(children(node)))
    raise RuntimeError('native tree not ready')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--pid', type=int, required=True)
    parser.add_argument('--app-log', type=Path, required=True)
    args = parser.parse_args()
    Atspi.init()
    try:
        def owned_tree():
            desktop = Atspi.get_desktop(0)
            for index in range(desktop.get_child_count()):
                app = desktop.get_child_at_index(index)
                if app.get_process_id() == args.pid:
                    # GTK can register its own widget hierarchy alongside
                    # AccessKit. Select the owned hierarchy with Kael's logical
                    # tree, rather than assuming the first matching PID owns it.
                    try:
                        return find_tree(app)
                    except Exception:
                        continue
            raise RuntimeError('owned logical tree not registered on the native accessibility bus')
        tree = wait(owned_tree)
        def projects_ready():
            require(children(tree) == 25, 'expected 25 native project roots')
            return [tree.get_child_at_index(index) for index in range(25)]
        projects = wait(projects_ready)
        total = 25
        for index, project in enumerate(projects):
            def descendants_ready():
                count = children(project)
                require(count == 4_000, f'project {index} lacks native offscreen descendants')
                return count
            total += wait(descendants_ready)
        require(total == 100_025, 'full logical tree is missing native rows')
        project = projects[24]
        require(project.get_name() == 'Project 25', 'native project order changed')
        last = project.get_child_at_index(3_999)
        require(last.get_name() == 'document_4000.rs', 'last offscreen native row missing')
        require(last.get_parent() == project, 'offscreen native parent mismatch')
        actions = last.get_action_iface()
        require(actions is not None, 'offscreen row lacks its native action interface')
        names = [actions.get_action_name(index) for index in range(actions.get_n_actions())]
        require('click' in names, f'last row does not advertise Click: {names}')
        # Upstream AccessKit AT-SPI currently exposes Click, while disclosure is
        # keyboard-controlled. This proof specifically verifies idle native Click.
        time.sleep(2)
        require(actions.do_action(names.index('click')), 'native click was rejected')
        def selected():
            text = args.app_log.read_text()
            for marker in ('NATIVE_ACCESSIBILITY_MODEL: rows=100025',
                           'NATIVE_ACCESSIBILITY_SELECT: id=100024'):
                require(marker in text, f'foreground did not execute {marker}')
            last.clear_cache()
            require(last.get_state_set().contains(Atspi.StateType.FOCUSED),
                    'native active descendant did not receive focus')
        wait(selected)
        print(f'NATIVE_ACCESSIBILITY_RUNTIME_OK: backend=atspi rows={total} '
              'projects=25 children=4000 last=100024 idle_select=true native_focus=true')
    finally:
        Atspi.exit()


if __name__ == '__main__':
    main()
