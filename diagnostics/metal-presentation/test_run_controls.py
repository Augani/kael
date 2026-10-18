"""Pure result-parser tests; no subprocess or native command is invoked."""
import importlib.util
from pathlib import Path
import unittest


def load_driver():
    path = Path(__file__).with_name('run-controls.py')
    spec = importlib.util.spec_from_file_location('metal_presentation_controls', path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


DRIVER = load_driver()


class ControlResultTests(unittest.TestCase):
    def summary(self, **changes):
        fields = {
            'mode': 'mtk',
            'transaction': '0',
            'handler_after_present': '0',
            'submitted': '60',
            'completed': '60',
            'callbacks': '60',
            'presented': '59',
            'dropped': '1',
            'failures': '0',
            'geometry': '1',
            'result': 'PASS',
        }
        fields.update(changes)
        return 'CONTROL_RESULT ' + ' '.join(f'{key}={value}' for key, value in fields.items())

    def test_genuine_positive(self):
        parsed, passed = DRIVER.parse_control(0, self.summary())
        self.assertTrue(passed)
        self.assertEqual(parsed['fields']['presented'], '59')

    def test_all_zero(self):
        # A forged PASS/zero exit cannot rescue zero submitted/complete work.
        _, passed = DRIVER.parse_control(0, self.summary(
            submitted='0', completed='0', callbacks='0', presented='0', dropped='0'))
        self.assertFalse(passed)

    def test_callback_not_presentation(self):
        _, passed = DRIVER.parse_control(0, self.summary(presented='0', dropped='60'))
        self.assertFalse(passed)

    def test_nonzero_exit(self):
        _, passed = DRIVER.parse_control(1, self.summary())
        self.assertFalse(passed)

    def test_gpu_incomplete(self):
        _, passed = DRIVER.parse_control(0, self.summary(completed='59'))
        self.assertFalse(passed)

    def test_gpu_error(self):
        _, passed = DRIVER.parse_control(0, self.summary(failures='1'))
        self.assertFalse(passed)

    def test_geometry_error(self):
        _, passed = DRIVER.parse_control(0, self.summary(geometry='0'))
        self.assertFalse(passed)

    def test_missing_result(self):
        parsed, passed = DRIVER.parse_control(0, 'CONTROL_COMPLETE frame=0 status=4\n')
        self.assertFalse(passed)
        self.assertEqual(parsed['summary_count'], 0)

    def test_duplicate_result(self):
        text = self.summary() + '\n' + self.summary()
        parsed, passed = DRIVER.parse_control(0, text)
        self.assertFalse(passed)
        self.assertEqual(parsed['summary_count'], 2)

    def test_inconsistent_callbacks(self):
        _, passed = DRIVER.parse_control(0, self.summary(dropped='0'))
        self.assertFalse(passed)

    def test_malformed_count(self):
        parsed, passed = DRIVER.parse_control(0, self.summary(submitted='not-a-count'))
        self.assertFalse(passed)
        self.assertTrue(parsed['malformed'])


if __name__ == '__main__':
    unittest.main()
