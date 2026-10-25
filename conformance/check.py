#!/usr/bin/env python3
"""Validate traceability and fixtures. Optional observations are runtime evidence."""
import argparse
from collections import Counter
import json
import math
from pathlib import Path
import sys

from inventory import ROOT, LEDGER, build, serialized, digest

CASES = ROOT / 'conformance/cases/portable.json'
KINDS = {'split', 'http_framing', 'multipart', 'websocket_close', 'error_redaction', 'limits'}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def validate_inventory(ledger):
    require(serialized(ledger) == serialized(build()), 'stale inventory; review and regenerate with inventory.py --write')
    ids = [r['id'] for r in ledger['records']]
    require(len(ids) == len(set(ids)), 'duplicate inventory IDs')
    for suite in ledger['suites']:
        require(suite['indexed_call_sites'] == suite['extracted_registrations'],
                f"{suite['source']}: registration count disagrees with pinned inventory")
        records = [r for r in ledger['records'] if r['source'] == suite['source'] and r['kind'] == 'registration']
        labels = Counter(r['label_ocaml'] for r in records if r['label_ocaml'] is not None)
        # Static inventory may omit dynamically constructed labels; each indexed
        # literal must still occur, with duplicate occurrences accounted for.
        for label, count in Counter(suite['indexed_labels']).items():
            require(labels[label] >= count, f"unaccounted indexed label: {suite['source']}: {label}")
    for record in ledger['records']:
        require(record['status'] in {'planned', 'deferred'}, 'static inventory cannot claim runtime passes')
        require(bool(record['reason']), 'missing disposition rationale')
    for vector in ledger['published_vector_files']:
        require((ROOT / vector['source']).is_file(), 'missing published upstream vector/license')
        require(digest(ROOT / vector['source']) == vector['indexed_sha256'], 'published upstream vector/license changed')


def validate_cases(value):
    require(value['version'] == 1 and bool(value['cases']), 'empty/unknown fixture format')
    ids = set()
    for case in value['cases']:
        require(case['id'] not in ids, 'duplicate case ID')
        ids.add(case['id'])
        require(case['kind'] in KINDS, 'unknown fixture kind')
        require(case['status'] == 'fixture_ready', 'fixture readiness is not a runtime pass')
        require(case['runtime_status']['ocaml-reference'] == 'planned', 'reference adapter not implemented')
        rust_status = case['runtime_status']['rust']
        require(rust_status in {'planned', 'implemented', 'implemented_partial'}, 'record runtime passes in observations, not fixtures')
        if rust_status in {'implemented', 'implemented_partial'}:
            if rust_status == 'implemented_partial':
                require(bool(case['divergence']), 'partial runner needs gap explanation')
            runner = case['runners']['rust']
            require((ROOT / runner['path']).is_file() and bool(runner['command']) and bool(runner['test']), 'implemented runner missing')
        require(case['timing'] == 'deterministic; no sleeps', 'timing contract missing')
        source = ROOT / case['source']['path']
        require(source.is_file(), 'missing case source')
        text = ' '.join(source.read_text().split())
        require(' '.join(case['source']['row'].split()) in text, 'source row missing: '+case['id'])
        require(bool(case['fragmentation']) and case['targets'] == ['ocaml-reference','rust'], 'missing execution model')
        if case['kind'] == 'split':
            bill, expected = case['input'], case['expected']
            require(set(bill) == {'total','tip','people'}, 'incomplete split input')
            invalid = []
            for key, minimum, maximum, integer in [('total',0,None,False),('tip',0,100,True),('people',1,50,True)]:
                number = bill[key]
                if (type(number) not in (int,float) or not math.isfinite(number)
                    or number < minimum or (maximum is not None and number > maximum)
                    or (integer and type(number) is not int)):
                    invalid.append(key)
            if invalid:
                require(expected == {'invalid_fields':invalid}, 'incorrect bounds expectation')
            else:
                require(set(expected) == {'each'} and math.isclose(expected['each'],bill['total']*(100+bill['tip'])/100/bill['people'],abs_tol=1e-12), 'incorrect split expectation')
        for key, data in case['input'].items():
            if key.endswith('_latin1'):
                data.encode('latin1')
            if key.endswith('_hex'):
                bytes.fromhex(data)
    return ids


def validate_observations(value, cases):
    """One observed result for every case per target. No silent partial pass."""
    require(value.get('version') == 1, 'unknown observations version')
    require(value.get('target') in {'ocaml-reference','rust'}, 'unknown observations target')
    require(bool(value.get('adapter')) and bool(value.get('revision')), 'adapter and tested revision are required')
    expected = {c['id']:c for c in cases['cases']}
    seen = set()
    counts = Counter()
    for result in value['results']:
        case_id = result['id']
        require(case_id in expected and case_id not in seen, 'unknown/duplicate result ID')
        seen.add(case_id)
        status = result['status']
        require(status in {'passed','failed','planned','deferred'}, 'unknown observation status')
        if status in {'passed','failed'}:
            require('observed' in result, 'executed result requires observed data')
            case = expected[case_id]
            if case['kind'] == 'error_redaction':
                observed = result['observed']
                public = json.dumps(observed['public'],ensure_ascii=False)
                matches = (all(s not in public for s in case['expected']['forbidden_public_substrings'])
                           and case['expected']['safe_message'] in public)
            else:
                matches = result['observed'] == case['expected']
            require((status == 'passed') == matches, 'claimed result disagrees with observation: '+case_id)
        else:
            require(bool(result.get('reason')), 'unexecuted case needs reason')
        counts[status] += 1
    require(seen == set(expected), 'observations omit cases; report planned/deferred explicitly')
    require(not counts['failed'], 'runtime observations contain failures')
    return dict(counts)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--require-executed-kind', action='append', choices=sorted(KINDS), default=[], help='Require all cases of this kind to have passed in supplied observations')
    parser.add_argument('--observations', type=Path, help='Optional complete target result file; format in README.md')
    args = parser.parse_args()
    try:
        ledger = json.loads(LEDGER.read_text())
        cases = json.loads(CASES.read_text())
        validate_inventory(ledger)
        ids = validate_cases(cases)
        print(f"Traceability/fixture validation passed: {len(ledger['suites'])} suites, {len(ledger['records'])} static entries, {len(ids)} portable cases.")
        if args.observations:
            observations = json.loads(args.observations.read_text())
            counts = validate_observations(observations, cases)
            results = {r['id']:r for r in observations['results']}
            for kind in args.require_executed_kind:
                require(all(results[c['id']]['status'] == 'passed' for c in cases['cases'] if c['kind'] == kind), 'required kind has unexecuted cases: '+kind)
            print('Runtime observations: '+json.dumps(counts,sort_keys=True))
        else:
            require(not args.require_executed_kind, '--require-executed-kind needs --observations')
            print('Runtime execution: not performed; no conformance passes claimed.')
    except (ValueError, KeyError, TypeError, OSError) as error:
        print('Conformance check failed: '+str(error),file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
