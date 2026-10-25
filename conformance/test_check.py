"""Regression tests for false-positive and stale-traceability rejection."""
import copy
import json
import unittest
from unittest.mock import patch

import check
from inventory import tokens


class CheckerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.cases = json.loads(check.CASES.read_text())

    def observation(self):
        return {'version':1, 'target':'rust', 'adapter':'test-adapter', 'revision':'test-revision',
                'results':[{'id':c['id'], 'status':'planned', 'reason':'Adapter absent.'} for c in self.cases['cases']]}

    def test_committed_cases(self):
        self.assertEqual(len(check.validate_cases(self.cases)),15)

    def test_invalid_bounds_cannot_be_mislabeled(self):
        cases = copy.deepcopy(self.cases)
        bad = next(c for c in cases['cases'] if c['id']=='split-invalid-all')
        bad['expected']['invalid_fields'] = ['people']
        with self.assertRaisesRegex(ValueError,'bounds'):
            check.validate_cases(cases)

    def test_literal_lexer_ignores_comments_and_characters(self):
        source = "(* nested (* test_case *) *) let ch = '\"' in row \"RFC\" \"says\" {|raw|}"
        strings = [t[1] for t in tokens(source) if t[0]=='string']
        self.assertEqual(strings,['RFC','says','raw'])
        self.assertFalse(any(t[1]=='test_case' for t in tokens(source)))

    def test_missing_results_are_rejected(self):
        report = self.observation()
        report['results'].pop()
        with self.assertRaisesRegex(ValueError,'omit'):
            check.validate_observations(report,self.cases)

    def test_false_pass_is_rejected(self):
        report = self.observation()
        report['results'][0] = {'id':'split-basic','status':'passed','observed':{'each':34}}
        with self.assertRaisesRegex(ValueError,'disagrees'):
            check.validate_observations(report,self.cases)

    def test_explicit_deferrals_are_counted(self):
        self.assertEqual(check.validate_observations(self.observation(),self.cases),{'planned':15})

    def test_one_real_observation_does_not_mark_every_case_passed(self):
        report = self.observation()
        report['results'][0] = {'id':'split-basic','status':'passed','observed':{'each':33.0}}
        self.assertEqual(check.validate_observations(report,self.cases),{'passed':1,'planned':14})

    def test_secret_in_public_error_fails(self):
        report = self.observation()
        result = next(r for r in report['results'] if r['id']=='error-redaction')
        result.update(status='passed',observed={'public':{'message':'An internal error occurred.', 'detail':'secret-conformance-canary'}})
        with self.assertRaisesRegex(ValueError,'disagrees'):
            check.validate_observations(report,self.cases)

    def test_failed_observations_fail_gate(self):
        report = self.observation()
        report['results'][0] = {'id':'split-basic','status':'failed','observed':{'each':34}}
        with self.assertRaisesRegex(ValueError,'failures'):
            check.validate_observations(report,self.cases)

    def test_stale_ledger_is_rejected(self):
        ledger = json.loads(check.LEDGER.read_text())
        changed = copy.deepcopy(ledger)
        changed['records'][0]['status'] = 'passed'
        with patch('check.build',return_value=ledger):
            with self.assertRaisesRegex(ValueError,'stale'):
                check.validate_inventory(changed)


if __name__ == '__main__':
    unittest.main()
