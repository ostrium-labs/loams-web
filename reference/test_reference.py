import unittest
from run_tests import classify


class VerdictTests(unittest.TestCase):
    def test_success_requires_count(self):
        self.assertEqual(classify(0,'Test Successful in 0.005s. 6 tests run.')['status'],'passed')
        self.assertEqual(classify(0,'all good')['status'],'unaccounted')
        self.assertEqual(classify(0,'0 tests run.')['status'],'unaccounted')

    def test_missing_service_skip_never_passes(self):
        result=classify(0,"[the issuer's rows skipped: no SPINDLE_TEST_OIDC]\n10 tests run.")
        self.assertEqual(result['status'],'incomplete')
        self.assertEqual(len(result['skip_evidence']),1)

    def test_skip_word_in_test_label_is_not_a_skip(self):
        self.assertEqual(classify(0,'[OK] a 1xx before final response is skipped\n4 tests run.')['status'],'passed')

    def test_colored_framework_skip_is_a_skip(self):
        result=classify(0,'\x1b[33m[SKIP]\x1b[0m issuer\n3 tests run.')
        self.assertEqual(result['status'],'incomplete')

    def test_nonzero_exit_always_fails_even_with_successful_test_summary(self):
        self.assertEqual(classify(124,'7 tests run.')['status'],'failed')


if __name__=='__main__':
    unittest.main()
