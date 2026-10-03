#!/usr/bin/env python3
"""External AT-SPI client for an owned Kael virtual-tree test process."""
import argparse
import os
from pathlib import Path
import time
import gi

gi.require_version('Atspi', '2.0')
from gi.repository import Atspi, Gio, GLib


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
    started = time.monotonic()
    count = node.get_child_count()
    if not 0 <= count <= 4_000:
        raise RuntimeError(f'native hierarchy exceeded fixture bounds: count={count}')
    if time.monotonic() - started > 1:
        print(f'NATIVE_ATSPI_QUERY: child_count={count} '
              f'seconds={time.monotonic() - started:.3f}', flush=True)
    return count


def native_identity(node):
    # libatspi disposes its cached GObject on Cache.RemoveAccessible. A fresh
    # Python wrapper on re-expansion is expected; AT-SPI identity is the stable
    # D-Bus service/path pair, not the client-side cache object's address.
    require(node.app is not None, 'native application identity missing')
    identity = (node.app.bus_name, node.path)
    require(isinstance(identity[0], str) and identity[0].startswith(':') and
            isinstance(identity[1], str) and identity[1].startswith('/'),
            f'invalid native object identity: {identity!r}')
    return identity


def trace_native_state(node):
    """Compare the retained libatspi proxy with one fresh native wire read."""
    bus, path = native_identity(node)
    connection = Gio.DBusConnection.new_for_address_sync(
        os.environ['AT_SPI_BUS_ADDRESS'],
        Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT |
        Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION, None, None)
    try:
        words, = connection.call_sync(
            bus, path, 'org.a11y.atspi.Accessible', 'GetState', None,
            GLib.VariantType.new('(au)'), Gio.DBusCallFlags.NONE, 3000, None).unpack()
        def present(state):
            ordinal = int(state)
            return bool(words[ordinal // 32] & (1 << (ordinal % 32)))
        node.clear_cache()
        cached = node.get_state_set()
        print('NATIVE_ATSPI_STATE_DIAGNOSTIC: '
              f'identity={(bus, path)!r} wire_words={words!r} '
              f'wire_focused={present(Atspi.StateType.FOCUSED)} '
              f'wire_focusable={present(Atspi.StateType.FOCUSABLE)} '
              f'wire_selected={present(Atspi.StateType.SELECTED)} '
              f'cached_defunct={cached.contains(Atspi.StateType.DEFUNCT)}', flush=True)
    finally:
        connection.close_sync(None)


def find_tree(app):
    # Search only the owned application's small top-level layout. Descendants
    # of the logical tree are explored separately, never scanned on the desktop.
    queue = [app]
    visited = 0
    while queue:
        node = queue.pop(0)
        visited += 1
        require(visited <= 512, 'native tree root not found within layout bound')
        if node.get_name() == 'Project files' and node.get_role() == Atspi.Role.TREE:
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
            desktop.clear_cache()
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
        print('NATIVE_ATSPI_STAGE: owned logical tree discovered', flush=True)
        def projects_ready():
            require(children(tree) == 25, 'expected 25 native project roots')
            return [tree.get_child_at_index(index) for index in range(25)]
        projects = wait(projects_ready)
        print('NATIVE_ATSPI_STAGE: 25 project roots discovered', flush=True)
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
        last_identity = native_identity(last)
        def action(node, name):
            node.clear_cache()
            interface = node.get_action_iface()
            require(interface is not None, f'native {name} interface missing')
            names = [interface.get_action_name(index)
                     for index in range(interface.get_n_actions())]
            require(name in names, f'native {name} capability missing: {names}')
            require(interface.do_action(names.index(name)), f'native {name} was rejected')

        state = project.get_state_set()
        require(state.contains(Atspi.StateType.EXPANDABLE), 'native branch is not expandable')
        require(state.contains(Atspi.StateType.EXPANDED), 'native branch lacks expanded state')
        time.sleep(2)
        action(project, 'collapse')
        def collapsed():
            require(children(project) == 0, 'collapsed branch still exposes documents')
            state = project.get_state_set()
            require(state.contains(Atspi.StateType.COLLAPSED), 'native collapsed state missing')
            require(not state.contains(Atspi.StateType.EXPANDED), 'native expanded state retained')
            require('NATIVE_ACCESSIBILITY_MODEL: rows=96025' in args.app_log.read_text(),
                    'foreground did not execute idle native collapse')
        wait(collapsed)
        try:
            stale = last.get_action_iface()
            require(stale is None or not stale.do_action(0),
                    'collapsed retained native document accepted an action')
        except GLib.GError:
            pass  # The removed D-Bus object correctly rejects the stale handle.
        action(project, 'expand')
        def expanded():
            require(children(project) == 4_000, 'native expand failed to restore descendants')
            require(project.get_state_set().contains(Atspi.StateType.EXPANDED),
                    'native expanded state did not return')
            require('NATIVE_ACCESSIBILITY_DISCLOSURE: id=96024 expanded=true'
                    in args.app_log.read_text(), 'foreground did not execute idle native expand')
            restored = project.get_child_at_index(3_999)
            require(native_identity(restored) == last_identity,
                    'native D-Bus document identity changed across disclosure')
            return restored
        last = wait(expanded)
        def restored_actions():
            # Parent children can return before the re-added D-Bus object's
            # interfaces are registered. The retained proxy may also cache the
            # absent interfaces it observed while the row was collapsed.
            last.clear_cache()
            actions = last.get_action_iface()
            require(actions is not None, 'offscreen row lacks its native action interface')
            names = [actions.get_action_name(index) for index in range(actions.get_n_actions())]
            require('click' in names, f'last row does not advertise Click: {names}')
            return actions, names
        actions, names = wait(restored_actions)
        time.sleep(2)
        require(actions.do_action(names.index('click')), 'native click was rejected')
        def selected():
            text = args.app_log.read_text()
            for marker in ('NATIVE_ACCESSIBILITY_MODEL: rows=100025',
                           'NATIVE_ACCESSIBILITY_SELECT: id=100024'):
                require(marker in text, f'foreground did not execute {marker}')
            last.clear_cache()
            states = last.get_state_set()
            require(states.contains(Atspi.StateType.FOCUSED),
                    'native active descendant did not receive focus: '
                    f'focusable={states.contains(Atspi.StateType.FOCUSABLE)} '
                    f'selected={states.contains(Atspi.StateType.SELECTED)}')
        try:
            wait(selected)
        except Exception:
            try:
                trace_native_state(last)
            except Exception as error:
                print(f'NATIVE_ATSPI_STATE_DIAGNOSTIC_ERROR: {error}', flush=True)
            raise
        print(f'NATIVE_ACCESSIBILITY_RUNTIME_OK: backend=atspi rows={total} '
              'projects=25 children=4000 last=100024 idle_select=true native_focus=true '
              'native_disclosure=true stable_identity=true')
    finally:
        Atspi.exit()


if __name__ == '__main__':
    main()
