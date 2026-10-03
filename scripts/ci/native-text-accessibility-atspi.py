#!/usr/bin/env python3
"""Real AT-SPI Text/EditableText and clipboard acceptance for an owned Editor.

Offsets are Unicode scalar indices (AT-SPI), InsertText lengths are UTF-8
bytes, and the application's diagnostic selection endpoints are UTF-8 bytes.
The protocol clients exercise native methods; the state log proves foreground
model changes and exactly one undo transaction per edit.
"""
import argparse
import json
from pathlib import Path
import subprocess
import time
import gi

gi.require_version('Atspi', '2.0')
from gi.repository import Atspi, GLib


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def wait(read, seconds=15):
    deadline = time.monotonic() + seconds
    while True:
        try:
            return read()
        except Exception:
            if time.monotonic() >= deadline:
                raise
            time.sleep(.05)


def state(path):
    records = [line[len('NATIVE_TEXT_STATE '):] for line in path.read_text().splitlines()
               if line.startswith('NATIVE_TEXT_STATE ')]
    require(records, 'foreground text state not published')
    return json.loads(records[-1])


def find_owned(pid, label):
    desktop = Atspi.get_desktop(0)
    desktop.clear_cache()
    failures = []
    roots = []
    for index in range(desktop.get_child_count()):
        try:
            app = desktop.get_child_at_index(index)
            app.clear_cache()
            app_pid = app.get_process_id()
            roots.append((getattr(getattr(app, 'app', None), 'bus_name', None), app_pid))
            if app_pid != pid:
                continue
            queue = [app]
            seen = 0
            while queue:
                node = queue.pop(0)
                seen += 1
                require(seen <= 512, 'owned editor layout exceeded bound')
                node.clear_cache()
                if node.get_name() == label:
                    return node
                count = node.get_child_count()
                require(0 <= count <= 512, f'unexpected editor hierarchy count={count}')
                queue.extend(node.get_child_at_index(i) for i in range(count))
        except (GLib.GError, RuntimeError) as error:
            # GTK and AccessKit may publish separate applications for this
            # same owned PID. A broken/stale GTK widget hierarchy must not
            # hide the independent Kael semantic hierarchy.
            failures.append(str(error))
    raise RuntimeError(f'owned native node not found: {label}; desktop_roots={roots!r}; errors={failures!r}')


def click(pid, label):
    node = wait(lambda: find_owned(pid, label))
    actions = node.get_action_iface()
    require(actions is not None, f'button has no native Action: {label}')
    names = [actions.get_action_name(i) for i in range(actions.get_n_actions())]
    require('click' in names, f'button has no Click: {label}')
    require(actions.do_action(names.index('click')), f'button action rejected: {label}')


def clipboard():
    return subprocess.run(['xclip', '-selection', 'clipboard', '-o'],
                          check=True, capture_output=True, timeout=3).stdout.decode('utf-8')


def bytes_at(content, offset):
    return len(content[:offset].encode('utf-8'))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--pid', type=int, required=True)
    parser.add_argument('--app-log', type=Path, required=True)
    parser.add_argument('--document', type=Path, required=True)
    parser.add_argument('--replacement', type=Path, required=True)
    args = parser.parse_args()
    # Path.read_text performs universal newline conversion; the fixture has a
    # deliberate CRLF, which must be preserved for all three offset domains.
    original = args.document.read_bytes().decode('utf-8')
    replacement = args.replacement.read_bytes().decode('utf-8')
    require(len(original.encode('utf-8')) == 105391 and len(original) == 63390,
            'canonical Unicode fixture changed')
    needle = '日本語 👩🏽\u200d💻 cafe\u0301'
    start = original.index(needle)
    end = start + len(needle)
    offscreen = original.index('KAEL_TEXT_OFFSCREEN_END')
    Atspi.init()
    try:
        document = wait(lambda: find_owned(args.pid, 'Native Unicode document'))
        readonly = wait(lambda: find_owned(args.pid, 'Read-only Unicode document'))
        def text_capabilities():
            # Semantic nodes can precede asynchronous D-Bus interface
            # registration. Wait for the complete provider, refreshing native
            # discovery rather than accepting an incomplete capability set.
            document.clear_cache()
            readonly.clear_cache()
            interfaces = (document.get_text_iface(), document.get_editable_text_iface(),
                          readonly.get_text_iface(), readonly.get_editable_text_iface())
            require(all(item is not None for item in interfaces),
                    'native Text/EditableText capability missing')
            return interfaces
        text, edit, ro_text, ro_edit = wait(text_capabilities)
        print('NATIVE_ATSPI_TEXT_STAGE: full native Text/EditableText discovered', flush=True)

        def contents(expected):
            document.clear_cache()
            require(Atspi.Text.get_character_count(text) == len(expected), 'native scalar count mismatch')
            require(Atspi.Text.get_text(text, 0, -1) == expected, 'native complete text mismatch')
            return state(args.app_log)

        wait(lambda: contents(original))
        require(Atspi.Text.get_text(ro_text, 0, -1) == original, 'read-only full text mismatch')
        require(Atspi.Text.set_selection(text, 0, start, end), 'native Unicode selection rejected')

        def selected():
            selection = Atspi.Text.get_selection(text, 0)
            require((selection.start_offset, selection.end_offset) == (start, end),
                    'native selected scalar range mismatch')
            require(Atspi.Text.get_text(text, start, end) == needle, 'native selected Unicode text mismatch')
            current = state(args.app_log)
            require((current['anchor'], current['focus']) ==
                    (bytes_at(original, start), bytes_at(original, end)),
                    'foreground selected byte range mismatch')
            return current

        selected_state = wait(selected)
        # Visible geometry is shaped, not a root-wide rectangle. The first
        # selected Japanese glyph is not itself a grapheme boundary ambiguity.
        rect = Atspi.Text.get_character_extents(text, start, Atspi.CoordType.SCREEN)
        require(rect.width > 0 and rect.height > 0, 'mounted text has no native geometry')
        hit = Atspi.Text.get_offset_at_point(text, rect.x + rect.width // 2,
                                             rect.y + rect.height // 2, Atspi.CoordType.SCREEN)
        require(hit == start, f'native glyph hit test mismatch: {hit} != {start}')
        word_offset = original.index('alpha_beta')
        word = Atspi.Text.get_string_at_offset(text, word_offset, Atspi.TextGranularity.WORD)
        require(word.start_offset <= word_offset < word.end_offset and word.content,
                'native word navigation missing')
        require(Atspi.Text.scroll_substring_to(text, offscreen, offscreen + 1, Atspi.ScrollType.TOP_EDGE),
                'native offscreen range reveal rejected')

        def revealed():
            rect = Atspi.Text.get_character_extents(text, offscreen, Atspi.CoordType.SCREEN)
            require(rect.width > 0 and rect.height > 0, 'revealed text has no native geometry')
            current = state(args.app_log)
            require((current['anchor'], current['focus']) ==
                    (selected_state['anchor'], selected_state['focus']),
                    'range reveal changed selection')
            component = document.get_component_iface().get_extents(Atspi.CoordType.SCREEN)
            require(component.y <= rect.y < component.y + component.height,
                    'offscreen range was not revealed into viewport')
        wait(revealed)
        require(Atspi.Text.set_caret_offset(text, len(original)), 'EOF native caret rejected')
        wait(lambda: require(state(args.app_log)['focus'] == len(original.encode('utf-8')),
                             'EOF caret byte mapping incorrect'))
        wait(lambda: require(Atspi.Text.get_caret_offset(text) == len(original),
                             'EOF native caret offset incorrect'))

        require(Atspi.EditableText.copy_text(ro_edit, start, end), 'read-only copy rejected')
        wait(lambda: require(clipboard() == needle, 'native Copy did not reach system clipboard'))
        require(not Atspi.EditableText.cut_text(ro_edit, start, end), 'read-only Cut falsely accepted')
        require(not Atspi.EditableText.paste_text(ro_edit, start), 'read-only Paste falsely accepted')
        require(not Atspi.EditableText.set_text_contents(ro_edit, 'invalid'), 'read-only SetTextContents accepted')
        require(Atspi.Text.get_text(ro_text, 0, -1) == original, 'read-only content changed')

        click(args.pid, 'Toggle disabled document')
        wait(lambda: require('NATIVE_TEXT_DISABLED: disabled=true' in args.app_log.read_text(),
                             'foreground did not disable editor'))
        def disabled():
            document.clear_cache()
            require(not document.get_state_set().contains(Atspi.StateType.ENABLED),
                    'disabled editor remains natively enabled')
        wait(disabled)
        require(not Atspi.Text.set_caret_offset(text, 0), 'disabled selection accepted')
        require(not Atspi.EditableText.insert_text(edit, 0, 'X', 1), 'disabled partial edit accepted')
        try:
            require(not Atspi.EditableText.copy_text(edit, start, end), 'disabled clipboard action accepted')
        except GLib.GError:
            pass  # void CopyText reports NotSupported on the D-Bus interface.
        require(Atspi.Text.get_text(text, 0, -1) == original, 'disabled editor mutated')
        click(args.pid, 'Toggle disabled document')
        def enabled():
            document.clear_cache()
            require(document.get_state_set().contains(Atspi.StateType.ENABLED),
                    'enabled editor capability did not recover')
        wait(enabled)

        def atomic_edit(operation, expected):
            before = state(args.app_log)
            require(operation(), 'native EditableText operation rejected')
            def changed():
                current = contents(expected)
                require(current['undo_depth'] == before['undo_depth'] + 1,
                        'native partial edit did not commit exactly one undo transaction')
                return current
            after = wait(changed)
            click(args.pid, 'Undo document')
            wait(lambda: contents(original))
            require(state(args.app_log)['undo_depth'] == before['undo_depth'],
                    'one Undo did not restore transaction depth')
            click(args.pid, 'Redo document')
            wait(lambda: contents(expected))
            require(state(args.app_log)['undo_depth'] == after['undo_depth'],
                    'one Redo did not restore transaction depth')
            click(args.pid, 'Undo document')
            wait(lambda: contents(original))

        atomic_edit(lambda: Atspi.EditableText.cut_text(edit, start, end), original[:start] + original[end:])
        wait(lambda: require(clipboard() == needle, 'Cut clipboard text mismatch'))
        atomic_edit(lambda: Atspi.EditableText.paste_text(edit, start), original[:start] + needle + original[start:])
        # GNOME specifies InsertText length in UTF-8 bytes. The prefix contains
        # two Japanese scalars (six bytes), with an extra emoji deliberately not inserted.
        atomic_edit(lambda: Atspi.EditableText.insert_text(edit, start, '日本🙂', 6),
                    original[:start] + '日本' + original[start:])
        atomic_edit(lambda: Atspi.EditableText.delete_text(edit, start, end), original[:start] + original[end:])
        require(not Atspi.EditableText.insert_text(edit, start, '日本', 2), 'partial UTF-8 insertion accepted')
        require(not Atspi.EditableText.delete_text(edit, end, start), 'reversed native edit range accepted')
        require(not Atspi.EditableText.delete_text(edit, 0, len(original) + 1), 'out-of-range edit accepted')
        require(Atspi.EditableText.set_text_contents(edit, 'whole 日本🙂\r\n'), 'whole SetTextContents rejected')
        wait(lambda: contents('whole 日本🙂\r\n'))
        click(args.pid, 'Reset document')
        wait(lambda: contents(original))
        click(args.pid, 'Replace document')
        wait(lambda: contents(replacement))
        require(Atspi.Text.get_text(ro_text, 0, -1) == original, 'independent read-only editor changed')
        click(args.pid, 'Native text checks complete')
        print('NATIVE_TEXT_ACCESSIBILITY_OK platform=linux '
              'utf8=105391 scalars=63390 utf16=66983 lines=1001 '
              'selection=true geometry=true hit_test=true words=true eof=true '
              'reveal=true clipboard=true editable=true atomic_undo=true '
              'readonly=true disabled=true replacement=true')
    finally:
        Atspi.exit()


if __name__ == '__main__':
    main()
