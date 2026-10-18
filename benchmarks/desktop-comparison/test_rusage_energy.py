"""Synthetic capability/aggregation tests. No process/native API."""
import copy
import errno
import unittest
from rusage_energy import summarize_rusage_energy as summary

def v6(e,p):
    return dict(rusage_flavor=6,rusage_v6_sdk_available=True,rusage_v6_errno=None,rusage_fallback_reason=None,energy_nj=e,penergy_nj=p)
def v4(sdk=True):
    return dict(rusage_flavor=4,rusage_v6_sdk_available=sdk,rusage_v6_errno=errno.EINVAL if sdk else None,rusage_fallback_reason='v6_einval' if sdk else 'sdk_without_v6',energy_nj=None,penergy_nj=None)

class RusageEnergyExtensionTests(unittest.TestCase):
    def test_full_window_endpoint_delta_is_not_sum_of_cumulative_counters(self):
        r=summary([v6(100,10),v6(150,30),v6(250,40)])
        self.assertIs(r['api_fields_available'],True)
        self.assertIs(r['energy_nj_capability'],True)
        self.assertEqual(r['energy_nj_delta'],150)
        self.assertEqual(r['penergy_nj_delta'],30)
        self.assertEqual(r['sample_count'],3)
        self.assertIs(r['power_or_hardware_calibration_established'],False)
    def test_old_sdk_and_unsupported_kernel_fallbacks_are_explicit(self):
        for sdk,reason in [(False,'sdk_without_v6'),(True,'v6_einval')]:
            with self.subTest(sdk=sdk):
                r=summary([v4(sdk),v4(sdk)])
                self.assertIs(r['api_fields_available'],False)
                self.assertIs(r['energy_nj_capability'],False)
                self.assertIsNone(r['energy_nj_delta'])
                self.assertIsNone(r['penergy_nj_delta'])
                self.assertEqual(r['observed_modes'],[reason])
    def test_legacy_schema3_has_unknown_capability(self):
        r=summary([{'billed_energy_raw':0},{'billed_energy_raw':1}])
        self.assertIsNone(r['api_fields_available'])
        self.assertIsNone(r['energy_nj_capability'])
        self.assertIsNone(r['energy_nj_delta'])
        self.assertEqual(r['capability_reason'],'legacy_no_metadata')
    def test_zero_filled_v6_is_unknown_not_zero_energy_evidence(self):
        r=summary([v6(0,0),v6(0,0)])
        self.assertIs(r['api_fields_available'],True)
        for key in ['energy_nj_capability','penergy_nj_capability','energy_nj_delta','penergy_nj_delta']:self.assertIsNone(r[key])
    def test_independent_field_capability_and_known_idle_zero_delta(self):
        r=summary([v6(0,100),v6(0,100)])
        self.assertIsNone(r['energy_nj_capability'])
        self.assertIsNone(r['energy_nj_delta'])
        self.assertIs(r['penergy_nj_capability'],True)
        self.assertEqual(r['penergy_nj_delta'],0)
    def test_mixed_window_never_aggregates_only_available_subset(self):
        for samples in ([v6(100,10),v4(),v6(300,30)],[{},v6(300,30)],[v4(False),v6(300,30)]):
            with self.subTest(samples=samples):
                r=summary(samples)
                self.assertIsNone(r['api_fields_available'])
                self.assertIsNone(r['energy_nj_delta'])
                self.assertIsNone(r['penergy_nj_delta'])
                self.assertEqual(r['capability_reason'],'mixed_collector_capability')
    def test_every_observation_is_monotonic_even_when_endpoints_increase(self):
        for key in ['energy_nj','penergy_nj']:
            samples=[v6(100,100),v6(150,150),v6(200,200)];samples[1][key]=50
            with self.subTest(key=key),self.assertRaisesRegex(RuntimeError,'non-monotonic'):summary(samples)
        with self.assertRaisesRegex(RuntimeError,'non-monotonic'):summary([v6(100,100),v4(),v6(50,200)])
    def test_non_unsigned_integer_counters_are_rejected(self):
        for key in ['energy_nj','penergy_nj']:
            for value in [None,True,-1,1.5,'2',2**64]:
                sample=v6(100,100);sample[key]=value
                with self.subTest(key=key,value=value),self.assertRaises(RuntimeError):summary([v6(100,100),sample])
    def test_partial_inconsistent_and_permission_error_metadata_is_rejected(self):
        bad=[v4()|{'rusage_v6_errno':errno.EPERM},v4()|{'rusage_v6_errno':errno.ESRCH},v4()|{'rusage_v6_errno':True},v4()|{'energy_nj':0},v6(100,100)|{'rusage_flavor':5},v6(100,100)|{'rusage_flavor':True},v6(100,100)|{'rusage_v6_errno':errno.EINVAL},v6(100,100)|{'rusage_v6_sdk_available':1},{'energy_nj':0}]
        for sample in bad:
            with self.subTest(sample=sample),self.assertRaises(RuntimeError):summary([v6(100,100),sample])
    def test_invalid_or_single_sample_window_is_not_a_delta(self):
        for samples in [None,[],[v6(100,100)],[None,None]]:
            with self.subTest(samples=samples),self.assertRaises(RuntimeError):summary(samples)
    def test_raw_samples_and_existing_counters_are_never_mutated(self):
        samples=[v6(100,10)|{'user_ns':123,'rss_bytes':900,'billed_energy_raw':41},v6(200,30)|{'user_ns':456,'rss_bytes':800,'billed_energy_raw':42}]
        original=copy.deepcopy(samples);summary(samples);self.assertEqual(samples,original)
if __name__=='__main__':unittest.main()
