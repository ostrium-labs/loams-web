#!/usr/bin/env python3
"""Inside-image runner: explicit suite verdicts and fail closed on skipped rows."""
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import time

SUITES = ['test_spindle','test_openapi','test_wire','test_http_rfc','test_websocket_rfc',
          'test_multipart_rfc','test_websocket','test_structured','test_spindle_postgres',
          'test_oauth_rfc','test_mcp_spec','test_cli','test_docs','test_spindle_sqlite','melange/test_client']
ANSI = re.compile(r'\x1b\[[0-?]*[ -/]*[@-~]')


def classify(returncode, output):
    text = ANSI.sub('',output)
    skipped = [line.strip() for line in text.splitlines() if re.search(r'\[\s*SKIP(?:PED)?\s*\]|^\s*\[[^\]]*\bskipped:',line)]
    summaries = re.findall(r'(\d+) tests? run\.',text)
    executed = sum(map(int,summaries))
    if returncode != 0:
        status = 'failed'
    elif skipped:
        status = 'incomplete'
    elif not summaries or executed == 0:
        status = 'unaccounted'
    else:
        status = 'passed'
    return {'status':status,'executed_tests':executed,'skip_evidence':skipped,'exit_code':returncode}


def capture(command, log, timeout=3600):
    started = time.monotonic()
    process = subprocess.Popen(command,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True,start_new_session=True)
    try:
        output,_ = process.communicate(timeout=timeout)
        code = process.returncode
    except subprocess.TimeoutExpired:
        os.killpg(process.pid,signal.SIGKILL)
        output,_ = process.communicate()
        code = 124
        output += '\nREFERENCE RUNNER TIMEOUT\n'
    log.write_text(output)
    return code,output,round(time.monotonic()-started,3)


def main():
    results = Path('/reports')
    results.mkdir(exist_ok=True)
    lock = json.loads(Path('/home/opam/reference-lock.json').read_text())
    report = {'version':1,'source_commit':os.environ['REFERENCE_SOURCE_COMMIT'],'services':{},'suites':[],
              'installed_packages':[],'status':'failed'}
    def save():
        (results/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    for name in ['SPINDLE_TEST_PG','SPINDLE_TEST_OIDC']:
        if not os.environ.get(name):
            report['failure']='Required service environment absent: '+name
            save()
            return 1
        report['services'][name] = 'configured'
    for tool in ['ocamlc','bun']:
        code,text,_ = capture([tool,'--version' if tool=='bun' else '-version'],results/(tool+'.log'))
        report[tool+'_version'] = text.strip()
        if code:
            report['failure']='Required tool unavailable: '+tool
            save()
            return 1
    code,text,_ = capture(['opam','list','--installed','--columns=name,version','--json=/reports/installed.json'],results/'installed.log')
    if code:
        report['failure']='Could not enumerate installed packages'
        save()
        return 1
    report['installed_packages'] = [{'name':line.split()[0],'version':line.split()[1]} for line in text.splitlines() if line and not line.startswith('#') and len(line.split())==2]
    installed = {p['name']:p['version'] for p in report['installed_packages']}
    for package in lock['direct_package_pins']:
        name,version=package.split('.',1)
        if installed.get(name)!=version:
            report['failure']='Installed version differs from lock: '+package
            save()
            return 1
    capture(['opam','switch','export','--full','--freeze','/reports/switch.export'],results/'switch-export.log')
    code,text,elapsed = capture(['dune','build','@all'],results/'build.log')
    report['build'] = {'exit_code':code,'elapsed_seconds':elapsed}
    if code:
        report['failure']='Reference build failed before test execution'
        save()
        return 1
    # Also include the Melange generated-client freshness rule.
    code,text,elapsed = capture(['dune','runtest','--force','--display=short'],results/'dune-runtest.log')
    report['runtest'] = {'exit_code':code,'elapsed_seconds':elapsed}
    for suite in SUITES:
        executable = 'test/'+suite+'.exe'
        code,text,elapsed = capture(['dune','exec',executable,'--','--color=never'],results/(suite.replace('/','-')+'.log'))
        source = Path('test')/(suite+'.ml')
        report['suites'].append({'name':suite,'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),
                                 'command':['dune','exec',executable,'--','--color=never'],
                                 'elapsed_seconds':elapsed,**classify(code,text)})
        save()
    report['executed_tests'] = sum(s['executed_tests'] for s in report['suites'])
    report['skipped_or_unaccounted_suites'] = sum(s['status'] in {'incomplete','unaccounted'} for s in report['suites'])
    report['status'] = 'passed' if report['runtest']['exit_code']==0 and all(s['status']=='passed' for s in report['suites']) else 'failed'
    save()
    return 0 if report['status']=='passed' else 1


if __name__=='__main__':
    sys.exit(main())
