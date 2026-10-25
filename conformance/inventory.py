#!/usr/bin/env python3
"""Deterministic static traceability; this never executes an upstream test."""
import argparse
import bisect
import hashlib
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
LEDGER = ROOT / 'conformance/inventory.json'


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def tokens(source):
    """Skip nested OCaml comments; retain source offsets and string literals.

    Labels are preserved in OCaml spelling, not evaluated. Generated test names
    and row input expressions require an actual reference adapter.
    """
    result = []
    quoted_pattern = re.compile(r'\{([a-z_]*)\|')
    ident_pattern = re.compile(r"[A-Za-z_][A-Za-z_0-9']*")
    char_pattern = re.compile(r"'(?:[^'\\\n]|\\(?:[0-9]{3}|x[0-9a-fA-F]{2}|o[0-7]{3}|.))'")
    i = 0
    while i < len(source):
        start = i
        if source.startswith('(*', i):
            depth = 1
            i += 2
            while i < len(source) and depth:
                if source.startswith('(*', i):
                    depth += 1
                    i += 2
                elif source.startswith('*)', i):
                    depth -= 1
                    i += 2
                else:
                    i += 1
            if depth:
                raise ValueError('unterminated comment')
            continue
        if source[i].isspace():
            i += 1
            continue
        char = char_pattern.match(source, i)
        if char:
            i = char.end()
            continue
        if source[i] == '"':
            i += 1
            while i < len(source):
                if source[i] == '\\':
                    i += 2
                elif source[i] == '"':
                    i += 1
                    break
                else:
                    i += 1
            result.append(('string', source[start + 1:i - 1], start))
            continue
        quoted = quoted_pattern.match(source, i)
        if quoted:
            end = source.find('|' + quoted[1] + '}', i + len(quoted[0]))
            if end < 0:
                raise ValueError('unterminated quoted string')
            result.append(('string', source[i + len(quoted[0]):end], start))
            i = end + len(quoted[1]) + 2
            continue
        ident = ident_pattern.match(source, i)
        if ident:
            i += len(ident[0])
            result.append(('identifier', ident[0], start))
        else:
            result.append(('symbol', source[i], start))
            i += 1
    return result


def build():
    upstream_path = ROOT / 'prds/loams-web-source-inventory.json'
    upstream = json.loads(upstream_path.read_text())
    records = []
    suites = []
    for suite in upstream['test_suites']:
        path = suite['path']
        source = (ROOT / path).read_text()
        ts = tokens(source)
        line_starts = [0] + [m.end() for m in re.finditer('\n', source)]
        deferred = any(x in path for x in ('mcp', 'oauth', 'postgres', 'sqlite', 'melange'))
        status = 'deferred' if deferred else 'planned'
        reason = ('Requires its package/runtime or service adapter; outside initial portable subset.'
                  if deferred else 'Awaiting executable reference and Rust adapters; static extraction is not a pass.')
        calls = []
        rows = []
        for index, token in enumerate(ts):
            kind, value, offset = token
            line = bisect.bisect_right(line_starts, offset)
            if kind != 'identifier':
                continue
            if value == 'test_case':
                following = ts[index + 1] if index + 1 < len(ts) else None
                label = following[1] if following and following[0] == 'string' else None
                calls.append({'line': line, 'label_ocaml': label})
            # Literal behavior tables may use row or crow (client response rows).
            if value in ('row', 'crow') and len(ts) > index + 2:
                a, b = ts[index + 1:index + 3]
                if a[0] == b[0] == 'string':
                    rows.append({'line': line, 'rfc': a[1], 'label_ocaml': b[1], 'constructor': value})
        for category, values in [('registration', calls), ('behavior_row', rows)]:
            for n, value in enumerate(values, 1):
                records.append({'id': f'{path}:{category}:{n}', 'source': path,
                                'kind': category, **value, 'status': status, 'reason': reason})
        suites.append({'source': path, 'source_sha256': digest(ROOT / path),
                       'indexed_call_sites': suite['test_case_call_sites'],
                       'indexed_labels': suite['literal_test_labels'],
                       'extracted_registrations': len(calls), 'extracted_literal_rows': len(rows),
                       'status': status, 'reason': reason,
                       'dynamic_expansion': 'Not evaluated: generated, parameterized and property tests remain owned by this suite.'})
    vectors = [f for f in upstream['files'] if f['path'].startswith('test/structured-field-tests/')]
    return {'version': 1, 'upstream_commit': upstream['upstream_commit'],
            'source_inventory_sha256': digest(upstream_path),
            'scope': 'All indexed suites and labels; lexical registrations and literal row/crow calls. Not execution or exhaustive dynamic expansion.',
            'suites': suites, 'records': records,
            'published_vector_files': [{'source': f['path'], 'indexed_sha256': f['sha256'],
                                       'status': 'planned', 'reason': 'Preserve upstream data/license; adapter not implemented.'} for f in vectors]}


def serialized(value):
    return json.dumps(value, ensure_ascii=False, indent=2) + '\n'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--write', action='store_true', help='Regenerate reviewed static ledger; never mark runtime passes.')
    args = parser.parse_args()
    expected = serialized(build())
    if args.write:
        LEDGER.write_text(expected)
    elif not LEDGER.exists() or LEDGER.read_text() != expected:
        raise SystemExit('conformance inventory is stale; review source changes and run python3 conformance/inventory.py --write')
    value = json.loads(expected)
    print(f"Inventory valid: {len(value['suites'])} suites, {len(value['records'])} static registrations/rows; runtime passes: 0")


if __name__ == '__main__':
    main()
