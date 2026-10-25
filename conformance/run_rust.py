#!/usr/bin/env python3
"""Run portable split cases against the actual Rust executable over TCP."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import selectors
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

from check import CASES, validate_cases, validate_observations

READY = re.compile(rb'^Bill split listening on http://127\.0\.0\.1:([0-9]+)$')


def readiness_line(line):
    match = READY.fullmatch(line.rstrip(b'\r'))
    if match:
        port = int(match[1])
        if 1 <= port <= 65535:
            return f'http://127.0.0.1:{port}'
    return None


def wait_ready(process, timeout=5):
    """Read a bounded stderr announcement; selector/deadline, never polling sleep."""
    deadline = time.monotonic() + timeout
    pending = b''
    captured = b''
    total = 0
    with selectors.DefaultSelector() as selector:
        selector.register(process.stderr, selectors.EVENT_READ)
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not selector.select(remaining):
                raise RuntimeError('Rust server did not announce readiness before deadline')
            chunk = os.read(process.stderr.fileno(), 4096)
            if not chunk:
                raise RuntimeError('Rust server exited before readiness: '+captured.decode('utf-8',errors='replace'))
            captured += chunk
            total += len(chunk)
            if total > 16384:
                raise RuntimeError('Rust server emitted excessive stderr before readiness')
            pending += chunk
            while b'\n' in pending:
                line, pending = pending.split(b'\n',1)
                address = readiness_line(line)
                if address:
                    return address


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=2)
    if process.stderr is not None:
        process.stderr.close()


def request_split(opener, address, bill):
    request = urllib.request.Request(address+'/split',data=json.dumps(bill,allow_nan=False).encode(),
                                     headers={'Content-Type':'application/json'},method='POST')
    try:
        response = opener.open(request,timeout=3)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        data = response.read(65537)
        if len(data) > 65536:
            raise RuntimeError('Response exceeded adapter limit')
        value = json.loads(data)
        if response.status != 200:
            return {'http_status':response.status,'json':value}
        return value


def normalize_split(observed):
    """Project actual structured problems into the reference's declared field order."""
    if not isinstance(observed, dict) or observed.get('http_status') != 400:
        return observed
    error = observed.get('json')
    if not isinstance(error, dict) or error.get('code') != 'invalid_argument' or error.get('reason') != 'invalid_input':
        return observed
    details = error.get('details')
    if not isinstance(details, dict) or details.get('truncated') is not False:
        return observed
    problems = details.get('problems')
    if not isinstance(problems, list) or not problems:
        return observed
    declared = ('total', 'tip', 'people')
    pointers = set()
    for problem in problems:
        if not isinstance(problem, dict) or problem.get('at') not in tuple('/'+field for field in declared):
            return observed
        pointers.add(problem['at'])
    return {'invalid_fields': [field for field in declared if '/'+field in pointers]}


def run(binary, cases):
    binary = Path(binary).resolve(strict=True)
    validate_cases(cases)
    revision = 'binary-sha256:'+hashlib.sha256(binary.read_bytes()).hexdigest()
    report = {'version':1,'target':'rust','adapter':'conformance/run_rust.py over loopback TCP',
              'revision':revision,'results':[]}
    env = dict(os.environ, LOAMS_WEB_ADDR='127.0.0.1:0')
    process = subprocess.Popen([str(binary)],env=env,stdin=subprocess.DEVNULL,
                               stdout=subprocess.DEVNULL,stderr=subprocess.PIPE)
    try:
        address = wait_ready(process)
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        for case in cases['cases']:
            result = {'id':case['id']}
            if case['kind']=='split':
                try:
                    observed = normalize_split(request_split(opener,address,case['input']))
                except (OSError,ValueError,RuntimeError) as error:
                    observed = {'adapter_error':str(error)}
                result.update(status='passed' if observed==case['expected'] else 'failed',observed=observed)
            else:
                result.update(status='planned',reason='Requires parser/controlled-I/O or application-specific adapter; split TCP runner does not execute this case.')
            report['results'].append(result)
    finally:
        stop(process)
    return report


def write_report(path, report):
    path = Path(path)
    # Atomic replacement keeps interrupted executions from leaving partial JSON.
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode='w',dir=path.parent,prefix=path.name+'.',delete=False) as stream:
            temporary = stream.name
            json.dump(report,stream,indent=2,allow_nan=False)
            stream.write('\n')
        os.replace(temporary,path)
    finally:
        if temporary is not None and os.path.exists(temporary):
            os.unlink(temporary)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    args = parser.parse_args()
    try:
        cases = json.loads(CASES.read_text())
        report = run(args.binary,cases)
        write_report(args.output,report)
        counts = validate_observations(report,cases)
        print(json.dumps(counts,sort_keys=True))
    except (OSError,ValueError,RuntimeError,subprocess.SubprocessError) as error:
        print('Rust observation adapter failed: '+str(error),file=sys.stderr)
        return 1
    return 0


if __name__=='__main__':
    sys.exit(main())
