"""Fault cases that must never become published performance evidence."""
import copy
import unittest

from measure import validate_workload


class ComparisonEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.phases = [
            {'phase': 'idle-before', 'elapsed_us': 1_000_000},
            {'phase': 'validation', 'elapsed_us': 6_000_000},
            {'phase': 'active', 'elapsed_us': 6_000_000},
            {'phase': 'validation', 'elapsed_us': 18_000_000},
            {'phase': 'idle-after', 'elapsed_us': 18_000_000},
            {'phase': 'validation', 'elapsed_us': 23_000_000},
            {'phase': 'churn', 'elapsed_us': 23_000_000},
            {'phase': 'validation', 'elapsed_us': 35_000_000},
            {'phase': 'finished', 'elapsed_us': 35_100_000},
        ]
        self.report = {
            'contract': 'native-editor-document-v1', 'engine': 'kael',
            'rows': 16_001, 'sections': 2_000, 'quick': False,
            'fixture_hash': 'fnv1a64:694401c508afd37d', 'fixture_bytes': 416_000,
            'font_family': 'Menlo', 'font_size_px': 14.0, 'line_height_px': 21.0,
            'window_width_px': 1_100, 'window_height_px': 760, 'syntax': 'plain',
            'theme_mode': 'dark',
            'phase_correctness': [[name, True] for name in
                                  ('idle-before', 'active', 'idle-after', 'churn')],
            'validation_phase_markers': True,
            'operations': dict.fromkeys(('selection', 'replace', 'undo', 'redo',
                                        'scroll_to_caret', 'home', 'replace_document'), 10),
            'frame_timing_enabled': True, 'elapsed_us': 35_100_000,
            'draw_cpu_us': [1, 20, 50], 'submission_cpu_us': [5, 10, 15],
            'first_submission_us': 500_000,
        }

    def validate(self, report=None, phases=None, enabled=True):
        validate_workload(self.report if report is None else report,
                          self.phases if phases is None else phases,
                          'kael', enabled, 'native-editor-document-v1', 16_001)

    def test_complete_capture_and_disabled_instrumentation_are_accepted(self):
        self.validate()
        disabled = copy.deepcopy(self.report)
        disabled.update(frame_timing_enabled=False, draw_cpu_us=[],
                        submission_cpu_us=[], first_submission_us=None)
        self.validate(disabled, enabled=False)

    def test_wrong_engine_fixture_typography_and_smoke_are_rejected(self):
        for key, value in (('engine', 'gpui-kit'), ('rows', 100_000),
                           ('fixture_hash', 'another-document'), ('fixture_bytes', 416_001),
                           ('font_size_px', 13.0), ('quick', True)):
            with self.subTest(key=key), self.assertRaises(RuntimeError):
                self.validate(self.report | {key: value})

    def test_missing_failed_or_reordered_document_checks_are_rejected(self):
        for checks in (self.report['phase_correctness'][:-1],
                       [['idle-before', False]] + self.report['phase_correctness'][1:],
                       list(reversed(self.report['phase_correctness']))):
            with self.subTest(checks=checks), self.assertRaises(RuntimeError):
                self.validate(self.report | {'phase_correctness': checks})

    def test_every_editor_operation_must_have_executed(self):
        for operation in self.report['operations']:
            missing = copy.deepcopy(self.report)
            missing['operations'][operation] = 0
            with self.subTest(operation=operation), self.assertRaises(RuntimeError):
                self.validate(missing)

    def test_phase_loss_duplicate_reorder_and_shortening_are_rejected(self):
        short = copy.deepcopy(self.phases)
        short[1]['elapsed_us'] = 2_000_000
        for phases in (self.phases[:-1], self.phases + self.phases[-1:],
                       list(reversed(self.phases)), short):
            with self.subTest(phases=phases), self.assertRaises(RuntimeError):
                self.validate(phases=phases)
        with self.assertRaises(RuntimeError):
            self.validate(self.report | {'elapsed_us': 24_000_000})

    def test_oracle_cpu_cannot_extend_a_short_measured_phase(self):
        slow_validation = copy.deepcopy(self.phases)
        slow_validation[1]['elapsed_us'] = 2_000_000
        # Five seconds until the next phase is insufficient if four of those
        # seconds were validation rather than the required idle observation.
        with self.assertRaises(RuntimeError):
            self.validate(phases=slow_validation)
        with self.assertRaises(RuntimeError):
            self.validate(self.report | {'validation_phase_markers': False})

    def test_mode_mismatch_and_missing_frame_measurements_are_rejected(self):
        for changes in ({'frame_timing_enabled': False}, {'frame_timing_enabled': 1},
                        {'draw_cpu_us': []}, {'submission_cpu_us': []},
                        {'first_submission_us': None}, {'first_submission_us': 40_000_000}):
            with self.subTest(changes=changes), self.assertRaises(RuntimeError):
                self.validate(self.report | changes)
        with self.assertRaises(RuntimeError):
            self.validate(enabled=False)

    def test_nonfinite_negative_and_unbounded_frame_captures_are_rejected(self):
        for values in ([float('nan')], [float('inf')], [-1], [True], [1] * 4097):
            with self.subTest(values=values[:2]), self.assertRaises(RuntimeError):
                self.validate(self.report | {'draw_cpu_us': values})

    def data_report(self):
        report = copy.deepcopy(self.report)
        report.update(
            contract='native-data-table-v1', rows=100_000,
            columns=['Title', 'Owner', 'Status', 'Score', 'Sprint', 'Updated', 'Tags', 'Notes'],
            column_count=8, fixture_hash='fnv1a64:4ea6da623acce6b5',
            final_hash='fnv1a64:4ea6da623acce6b5',
            fixture_bytes=12_166_666, fixture_retained_datasets=2,
            font_size_px=13.0, header_font_size_px=12.0, line_height_px=20.0,
            row_height_px=28.0, column_width_px=112.0,
            table_width_px=780.0, table_height_px=704, fixed_columns=1,
            header_height_px=32, leaf_header_height_px=32,
            native_cell_editing=False, cell_wrap='nowrap',
            operations=dict.fromkeys(('selection_reveal', 'vertical_scroll', 'horizontal_scroll',
                                      'query_reverse', 'home', 'end_selection', 'replace_model'), 10),
            phase_oracles=[{
                'phase': phase, 'correct': True, 'verified_model_cells': 800_000,
                'verified_control_cells': 8, 'dataset_generation': 0,
                'query_reversed': False, 'selected_cell': [0, 0],
                'verified_control_row': 0, 'ordered_hash': 'fnv1a64:4ea6da623acce6b5',
                'native_viewport_rows': 24, 'native_viewport_scrollable_columns': 6,
            } for phase in ('idle-before', 'active', 'idle-after', 'churn')],
        )
        return report

    def validate_data(self, report, engine='kael'):
        validate_workload(report, self.phases, engine, True, 'native-data-table-v1', 100_000)

    def test_data_native_geometry_and_oracles_are_accepted_for_both_engines(self):
        report = self.data_report()
        self.validate_data(report)
        report.update(engine='gpui-kit', leaf_header_height_px=28)
        self.validate_data(report, engine='gpui-kit')

    def test_data_missing_cells_stale_identity_and_unbounded_viewport_are_rejected(self):
        mutations = (
            ('correct', False), ('verified_model_cells', 799_999), ('verified_control_cells', 7),
            ('dataset_generation', 2), ('dataset_generation', True), ('query_reversed', 1),
            ('selected_cell', [100_000, 0]), ('selected_cell', [0, 8]),
            ('selected_cell', [False, 0]), ('selected_cell', None),
            ('verified_control_row', -1), ('verified_control_row', 100_000),
            ('ordered_hash', 'stale'), ('ordered_hash', 'fnv1a64:cc17af116933398f'),
            ('native_viewport_rows', 0),
            ('native_viewport_rows', 65), ('native_viewport_scrollable_columns', 9),
        )
        for key, value in mutations:
            report = self.data_report()
            report['phase_oracles'][2][key] = value
            with self.subTest(key=key, value=value), self.assertRaises(RuntimeError):
                self.validate_data(report)
        for oracles in (None, [], self.data_report()['phase_oracles'][:-1]):
            report = self.data_report()
            report['phase_oracles'] = oracles
            with self.subTest(oracles=oracles), self.assertRaises(RuntimeError):
                self.validate_data(report)
        with self.assertRaises(RuntimeError):
            self.validate_data(self.data_report() | {'final_hash': 'fnv1a64:cc17af116933398f'})

    def test_data_altered_geometry_theme_and_unexecuted_operations_are_rejected(self):
        for key, value in (('table_width_px', 896), ('fixed_columns', 0),
                           ('line_height_px', 21), ('theme_mode', 'light'),
                           ('leaf_header_height_px', 28), ('fixture_retained_datasets', 1),
                           ('fixed_columns', True)):
            report = self.data_report()
            report[key] = value
            with self.subTest(key=key), self.assertRaises(RuntimeError):
                self.validate_data(report)
        for operation in self.data_report()['operations']:
            report = self.data_report()
            report['operations'][operation] = 0
            with self.subTest(operation=operation), self.assertRaises(RuntimeError):
                self.validate_data(report)

    def tree_report(self):
        from workload_validation import TREE_HASHES
        report = copy.deepcopy(self.report)
        report.update(
            contract='native-virtual-tree-v1', rows=100025, root_count=25,
            children_per_root=4000, fixture_hash=TREE_HASHES[0], fixture_bytes=6100975,
            fixture_hashes=TREE_HASHES, fixture_retained_datasets=2,
            replacement_interval_updates=30, tree_width_px=780, tree_height_px=704,
            row_height_px=28, indent_px=14, horizontal_scroll=False,
            line_height_px=20, component='VirtualTreeList',
            operations=dict.fromkeys(('selection_reveal', 'vertical_scroll', 'collapse_root',
                                      'expand_root', 'home', 'end_selection', 'replace_model'), 10),
            phase_oracles=[{
                'phase': phase, 'correct': True, 'dataset_generation': 0,
                'selected_id': 'project/00', 'scroll_target_id': 'project/00',
                'collapsed_root': None, 'visible_nodes': 100025,
                'verified_control_nodes': 100025, 'visible_hash': TREE_HASHES[0],
                'fixture_hash': TREE_HASHES[0], 'native_mounted_rows': 26,
                'mounted_indices': list(range(26)), 'reveal_target_mounted': True,
            } for phase in ('idle-before', 'active', 'idle-after', 'churn')],
        )
        return report

    def validate_tree_report(self, report, engine='kael'):
        validate_workload(report, self.phases, engine, True, 'native-virtual-tree-v1', 100025)

    def test_tree_fixture_hash_and_both_native_controls_are_accepted(self):
        report = self.tree_report()
        self.validate_tree_report(report)
        report.update(engine='gpui-kit', component='Tree')
        self.validate_tree_report(report, 'gpui-kit')

    def test_tree_stale_projection_and_unmounted_reveal_are_rejected(self):
        for key, value in (('dataset_generation', True), ('collapsed_root', 25),
                           ('collapsed_root', 0), ('visible_nodes', 100024),
                           ('verified_control_nodes', 100024), ('visible_hash', 'stale'),
                           ('selected_id', 'project/25'), ('scroll_target_id', 'project/00/file/0100'),
                           ('scroll_target_id', 'project/00/file/4000'),
                           ('native_mounted_rows', 65), ('mounted_indices', [0, 0]),
                           ('mounted_indices', list(range(65))), ('reveal_target_mounted', False)):
            report = self.tree_report()
            report['phase_oracles'][2][key] = value
            with self.subTest(key=key, value=value), self.assertRaises(RuntimeError):
                self.validate_tree_report(report)
        for key, value in (('root_count', 24), ('indent_px', 16),
                           ('fixture_bytes', 6100976), ('component', 'custom-tree')):
            with self.subTest(key=key), self.assertRaises(RuntimeError):
                self.validate_tree_report(self.tree_report() | {key: value})
        for operation in self.tree_report()['operations']:
            report = self.tree_report()
            report['operations'][operation] = 0
            with self.subTest(operation=operation), self.assertRaises(RuntimeError):
                self.validate_tree_report(report)

    def workspace_report(self):
        from workload_validation import PANE_IDS, WORKSPACE_HASHES
        report = copy.deepcopy(self.report)
        groups = [{'Tabs': {'panes': PANE_IDS[start:start + 4], 'active': PANE_IDS[start]}}
                  for start in (0, 4, 8)]
        layout = {'Split': {'axis': 'horizontal', 'children': [groups[0],
                           {'Split': {'axis': 'vertical', 'children': groups[1:]}}]}}
        report.update(
            contract='native-dock-workspace-v1', rows=12, pane_count=12, body_lines_per_pane=32,
            fixture_hash=WORKSPACE_HASHES[0], fixture_bytes=22560,
            fixture_hashes=WORKSPACE_HASHES, fixture_retained_datasets=2,
            replacement_interval_updates=32, workspace_width_px=1100, workspace_height_px=704,
            initial_tab_groups=3, initial_split_count=2,
            initial_split_axes=['horizontal', 'vertical'], initial_split_ratios=[0.5, 0.5],
            native_chrome_geometry=True, floating_panes=False, edge_docks=False,
            line_height_px=20, component='DockWorkspace',
            operations=dict.fromkeys(('select_tab', 'move_tab', 'split_pane', 'merge_pane',
                                      'resize_split', 'zoom_group', 'unzoom_group',
                                      'serialize_restore', 'replace_model'), 10),
            phase_oracles=[{
                'phase': phase, 'correct': True, 'fixture_generation': 0,
                'fixture_hash': WORKSPACE_HASHES[0], 'actual_layout': copy.deepcopy(layout),
                'verified_pane_ids': PANE_IDS, 'verified_active_content': ['pane-00', 'pane-04', 'pane-08'],
                'native_zoomed': False, 'root_split_ratio': 0.5,
                'persistence_roundtrips_verified': index, 'native_layout_json_bytes': 458,
                'content_verified': True,
            } for index, phase in enumerate(('idle-before', 'active', 'idle-after', 'churn'))],
        )
        return report

    def validate_workspace_report(self, report, engine='kael'):
        validate_workload(report, self.phases, engine, True, 'native-dock-workspace-v1', 12)

    def test_workspace_native_layouts_and_zoom_are_accepted_for_both_engines(self):
        report = self.workspace_report()
        self.validate_workspace_report(report)
        report.update(engine='gpui-kit', component='DockArea+DockSkin')
        report['phase_oracles'][1].update(native_zoomed=True, verified_active_content=['pane-00'])
        self.validate_workspace_report(report, 'gpui-kit')

    def test_workspace_lost_panes_stale_content_and_fake_persistence_are_rejected(self):
        for key, value in (('fixture_generation', 2), ('fixture_hash', 'stale'),
                           ('verified_pane_ids', ['pane-00']), ('verified_active_content', []),
                           ('content_verified', False), ('native_zoomed', 1),
                           ('root_split_ratio', float('nan')), ('root_split_ratio', True),
                           ('persistence_roundtrips_verified', -1),
                           ('native_layout_json_bytes', 0), ('actual_layout', None)):
            report = self.workspace_report()
            report['phase_oracles'][2][key] = value
            with self.subTest(key=key), self.assertRaises(RuntimeError):
                self.validate_workspace_report(report)
        report = self.workspace_report()
        report['phase_oracles'][1]['actual_layout']['Split']['children'][0]['Tabs']['panes'][1] = 'pane-00'
        with self.assertRaises(RuntimeError):
            self.validate_workspace_report(report)
        report = self.workspace_report()
        for oracle in report['phase_oracles']:
            oracle['persistence_roundtrips_verified'] = 0
        with self.assertRaises(RuntimeError):
            self.validate_workspace_report(report)
        for operation in self.workspace_report()['operations']:
            report = self.workspace_report()
            report['operations'][operation] = 0
            with self.subTest(operation=operation), self.assertRaises(RuntimeError):
                self.validate_workspace_report(report)



if __name__ == '__main__':
    unittest.main()
