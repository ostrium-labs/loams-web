#!/usr/bin/env python3
"""Prepare exact retained source, build reference image, and run all services/tests."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import time
import uuid

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
WORK = HERE/'.work'
RESULTS = HERE/'results'
LOCK = json.loads((HERE/'lock.json').read_text())
IMAGE = 'localhost/loams-web-reference:'+LOCK['source_commit'][:12]


def save(value):
    RESULTS.mkdir(exist_ok=True)
    (RESULTS/'attempt.json').write_text(json.dumps(value,indent=2)+'\n')


def command(args, report, label, timeout=3600):
    log = RESULTS/(label+'.log')
    started = time.monotonic()
    with log.open('w') as stream:
        try:
            result = subprocess.run(args,cwd=ROOT,stdout=stream,stderr=subprocess.STDOUT,timeout=timeout)
            code = result.returncode
        except subprocess.TimeoutExpired:
            code = 124
            stream.write('\nREFERENCE COMMAND TIMEOUT\n')
    report['steps'].append({'name':label,'command':args,'exit_code':code,
                            'elapsed_seconds':round(time.monotonic()-started,3),'log':str(log)})
    save(report)
    if code:
        raise RuntimeError(label+' failed; see '+str(log))


def prepare():
    WORK.mkdir(exist_ok=True)
    source = WORK/'source'
    if source.exists():
        raise RuntimeError('Prepared source already exists; use build/run or choose a fresh work directory after reviewing it')
    archive = WORK/'source.tar'
    with archive.open('wb') as stream:
        subprocess.run(['git','archive','--format=tar',LOCK['source_commit']],cwd=ROOT,stdout=stream,check=True)
    source.mkdir()
    with tarfile.open(archive) as tar:
        tar.extractall(source,filter='data')
    shutil.copy2(HERE/'Containerfile',WORK/'Containerfile')
    shutil.copy2(HERE/'run_tests.py',WORK/'run_tests.py')
    shutil.copy2(HERE/'lock.json',WORK/'lock.json')
    value = {'source_commit':LOCK['source_commit'],'archive_sha256':hashlib.sha256(archive.read_bytes()).hexdigest(),
             'files':[{'path':str(p.relative_to(source)),'sha256':hashlib.sha256(p.read_bytes()).hexdigest()} for p in sorted(source.rglob('*')) if p.is_file()]}
    (WORK/'source-manifest.json').write_text(json.dumps(value,indent=2)+'\n')
    print('Prepared '+str(source))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action',choices=['prepare','build','run'])
    args = parser.parse_args()
    if args.action=='prepare':
        prepare()
        return 0
    RESULTS.mkdir(exist_ok=True)
    report = {'version':1,'action':args.action,'lock':LOCK,'steps':[],'status':'failed',
              'installed_package_count':0,'executed_test_count':0,'test_execution':'not started'}
    if (WORK/'source-manifest.json').is_file():
        report['source_archive_sha256']=json.loads((WORK/'source-manifest.json').read_text())['archive_sha256']
    save(report)
    network = 'loams-web-reference-'+uuid.uuid4().hex[:12]
    db = network+'-db'
    created_network = False
    created_db = False
    try:
        if args.action=='build':
            # Refresh harness copies without touching archived upstream files.
            shutil.copy2(HERE/'Containerfile',WORK/'Containerfile')
            shutil.copy2(HERE/'run_tests.py',WORK/'run_tests.py')
            shutil.copy2(HERE/'lock.json',WORK/'lock.json')
            report['harness_sha256']={name:hashlib.sha256((WORK/name).read_bytes()).hexdigest() for name in ['Containerfile','run_tests.py','lock.json']}
            save(report)
            command(['podman','build','--platform',LOCK['platform'],'--tag',IMAGE,'--file',str(WORK/'Containerfile'),str(WORK)],report,'build',timeout=14400)
            command(['podman','image','inspect',IMAGE],report,'image-inspect',timeout=30)
            report['status']='built'
        else:
            (RESULTS/'report.json').unlink(missing_ok=True)
            command(['podman','network','create',network],report,'network',timeout=60)
            created_network = True
            created_db = True
            command(['podman','run','--detach','--name',db,'--network',network,'--network-alias','db',
                     '--env','POSTGRES_USER=spindle','--env','POSTGRES_PASSWORD=spindle','--env','POSTGRES_DB=spindle',
                     '--tmpfs','/var/lib/postgresql','--health-cmd','pg_isready -U spindle -d spindle',
                     '--health-interval','1s','--health-retries','30',LOCK['postgres_image'],
                     'postgres','-c','fsync=off','-c','synchronous_commit=off','-c','full_page_writes=off'],report,'postgres',timeout=600)
            command(['podman','wait','--condition','healthy',db],report,'postgres-ready',timeout=90)
            command(['podman','run','--rm','--userns','keep-id','--network',network,
                     '--env','SPINDLE_TEST_PG=postgres://spindle:spindle@db:5432/spindle',
                     '--env','SPINDLE_TEST_OIDC=/home/opam/spindle/test/oidc',
                     '--volume',str(RESULTS)+':/reports:rw',IMAGE],report,'tests',timeout=14400)
            test_report=json.loads((RESULTS/'report.json').read_text())
            report.update(status=test_report['status'],test_execution='completed',
                          installed_package_count=len(test_report['installed_packages']),
                          executed_test_count=test_report['executed_tests'])
    except (OSError,RuntimeError,subprocess.SubprocessError,KeyError) as error:
        report['failure']=str(error)
        # Preserve partial inner report even when the container exits nonzero.
        inner=RESULTS/'report.json'
        if args.action=='run' and inner.exists():
            value=json.loads(inner.read_text())
            report.update(test_execution='attempted',installed_package_count=len(value.get('installed_packages',[])),
                          executed_test_count=value.get('executed_tests',0))
        print(error,file=sys.stderr)
        return 1
    finally:
        for cleanup in ([['podman','rm','--force',db]] if created_db else []) + ([['podman','network','rm',network]] if created_network else []):
            try:
                result=subprocess.run(cleanup,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=30)
                if result.returncode:
                    report.setdefault('cleanup_errors',[]).append(cleanup)
            except (OSError,subprocess.TimeoutExpired):
                report.setdefault('cleanup_errors',[]).append(cleanup)
        save(report)
    print(report['status'])
    return 0


if __name__=='__main__':
    sys.exit(main())
