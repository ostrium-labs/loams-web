"""Readiness and cleanup checks without real ports or timing-dependent sleeps."""
import json
import os
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import run_rust


class RustAdapterTests(unittest.TestCase):
    def test_normalization_uses_observed_fields_in_declared_order(self):
        observed = {'http_status': 400, 'json': {
            'code': 'invalid_argument', 'reason': 'invalid_input',
            'details': {'truncated': False, 'problems': [
                {'at': '/people'}, {'at': '/total'}, {'at': '/people'}]}}}
        self.assertEqual(run_rust.normalize_split(observed), {'invalid_fields': ['total', 'people']})
        self.assertEqual(run_rust.normalize_split({'each': 33.0}), {'each': 33.0})

    def test_normalization_never_fabricates_missing_field_evidence(self):
        for details in [None, {'problems': []},
                        {'truncated': True, 'problems': [{'at': '/people'}]},
                        {'truncated': False, 'problems': [{'at': '/secret-key'}]},
                        {'truncated': False, 'problems': [{'at': ''}]}]:
            observed = {'http_status': 400, 'json': {
                'code': 'invalid_argument', 'reason': 'invalid_input', 'details': details}}
            self.assertEqual(run_rust.normalize_split(observed), observed)

    def test_readiness_rejects_non_loopback_and_invalid_port(self):
        self.assertIsNone(run_rust.readiness_line(b'Bill split listening on http://0.0.0.0:1234'))
        self.assertIsNone(run_rust.readiness_line(b'Bill split listening on http://127.0.0.1:0'))
        self.assertIsNone(run_rust.readiness_line(b'Bill split listening on http://127.0.0.1:65536'))
        self.assertEqual(run_rust.readiness_line(b'Bill split listening on http://127.0.0.1:1234'),'http://127.0.0.1:1234')

    def pipe_process(self,data):
        read_fd, write_fd = os.pipe()
        os.write(write_fd,data)
        os.close(write_fd)
        stream = os.fdopen(read_fd,'rb')
        self.addCleanup(stream.close)
        return SimpleNamespace(stderr=stream)

    def test_readiness_reads_announcement_after_other_lines(self):
        process = self.pipe_process(b'initializing\nBill split listening on http://127.0.0.1:4321\n')
        self.assertEqual(run_rust.wait_ready(process),'http://127.0.0.1:4321')

    def test_startup_errors_keep_diagnostic(self):
        process = self.pipe_process(b'Permission denied\n')
        with self.assertRaisesRegex(RuntimeError,'Permission denied'):
            run_rust.wait_ready(process)

    def test_startup_timeout_always_stops_process(self):
        cases = json.loads(run_rust.CASES.read_text())
        with patch('run_rust.subprocess.Popen') as popen, patch('run_rust.wait_ready',side_effect=RuntimeError('timeout')), patch('run_rust.stop') as stop:
            with self.assertRaisesRegex(RuntimeError,'timeout'):
                run_rust.run(Path(__file__),cases)
            stop.assert_called_once_with(popen.return_value)


if __name__=='__main__':
    unittest.main()
