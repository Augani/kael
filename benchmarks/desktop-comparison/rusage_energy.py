"""Additive schema3 rusage energy extension. Pure aggregation; never samples a process."""
import errno

EXTRA_KEYS = frozenset(('rusage_flavor', 'rusage_v6_sdk_available', 'rusage_v6_errno',
                        'rusage_fallback_reason', 'energy_nj', 'penergy_nj'))


def summarize_rusage_energy(samples):
    """Keep unavailable/unknown distinct from zero; reject malformed or decreasing data."""
    if not isinstance(samples, list) or len(samples) < 2:
        raise RuntimeError('energy aggregation requires the original stable sample window')
    kinds = []
    values = {'energy_nj': [], 'penergy_nj': []}
    for sample in samples:
        if not isinstance(sample, dict):
            raise RuntimeError('invalid process sample for energy extension')
        present = EXTRA_KEYS.intersection(sample)
        if not present:
            kinds.append('legacy_no_metadata')
            continue
        if present != EXTRA_KEYS:
            raise RuntimeError('partial rusage energy metadata')
        flavor = sample['rusage_flavor']
        sdk = sample['rusage_v6_sdk_available']
        failure = sample['rusage_v6_errno']
        reason = sample['rusage_fallback_reason']
        if type(flavor) is not int or flavor not in (4, 6) or type(sdk) is not bool:
            raise RuntimeError('invalid rusage flavor or SDK capability')
        if flavor == 4:
            valid_fallback = ((sdk is False and failure is None and reason == 'sdk_without_v6')
                              or (sdk is True and type(failure) is int
                                  and failure == errno.EINVAL and reason == 'v6_einval'))
            if not valid_fallback or any(sample[key] is not None for key in values):
                raise RuntimeError('invalid explicit V4 fallback')
            kinds.append(reason)
            continue
        if sdk is not True or failure is not None or reason is not None:
            raise RuntimeError('inconsistent V6 capability metadata')
        kinds.append('v6_fields_returned')
        for key, previous in values.items():
            value = sample[key]
            if type(value) is not int or not 0 <= value < 2**64:
                raise RuntimeError('invalid unsigned rusage energy counter')
            if previous and value < previous[-1]:
                raise RuntimeError('non-monotonic rusage energy counter')
            previous.append(value)
    modes = sorted(set(kinds))
    complete_v6 = modes == ['v6_fields_returned']
    legacy = modes == ['legacy_no_metadata']
    v4 = bool(modes) and set(modes) <= {'sdk_without_v6', 'v6_einval'}
    available = True if complete_v6 else (False if v4 else None)
    result = {'extension_version': 1, 'sample_count': len(samples),
              'api_fields_available': available,
              'capability_reason': ('v6_fields_returned' if complete_v6 else
                                    ('legacy_no_metadata' if legacy else
                                     ('v4_fallback' if v4 else 'mixed_collector_capability'))),
              'observed_modes': modes, 'units': 'OS-reported nanjoules',
              'power_or_hardware_calibration_established': False}
    for key, observed in values.items():
        # Successful zero-filled V6 fields do not establish hardware capability.
        # Raw zero samples are still preserved unchanged in process_samples.
        capability = (True if any(observed) else None) if complete_v6 else (False if v4 else None)
        result[key + '_capability'] = capability
        result[key + '_delta'] = observed[-1] - observed[0] if capability is True else None
    return result
