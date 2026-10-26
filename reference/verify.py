#!/usr/bin/env python3
"""Check checked-in reference pins and optional extracted source integrity."""
import hashlib
import json
from pathlib import Path
import re
import sys

HERE=Path(__file__).resolve().parent


def main():
    lock=json.loads((HERE/'lock.json').read_text())
    containerfile=(HERE/'Containerfile').read_text()
    for key in ['ocaml_image','bun_image','default_repository','hypha_repository']:
        assert lock[key] in containerfile, 'Containerfile differs from '+key
    for key in ['ocaml_image','bun_image','postgres_image']:
        assert re.fullmatch(r'[^\s]+@sha256:[0-9a-f]{64}',lock[key]), 'Mutable/invalid image reference: '+key
    for key in ['default_repository','hypha_repository']:
        assert re.fullmatch(r'https://github.com/[^\s]+\.git#[0-9a-f]{40}',lock[key]), 'Unpinned repository: '+key
    for package in lock['direct_package_pins']:
        assert package in containerfile, 'Missing version pin: '+package
    assert lock['source_commit'] in containerfile
    for package in ['wiretype.0.6.0','rowtype.0.9.0']:
        text=(HERE/(package+'.opam')).read_text()
        assert re.search(r'sha256=[0-9a-f]{64}',text)
        assert re.search(r'x-commit-hash: "[0-9a-f]{40}"',text)
    manifest=HERE/'.work/source-manifest.json'
    if manifest.exists():
        value=json.loads(manifest.read_text())
        assert value['source_commit']==lock['source_commit']
        archive=HERE/'.work/source.tar'
        assert hashlib.sha256(archive.read_bytes()).hexdigest()==value['archive_sha256'], 'Archived source changed'
        for entry in value['files']:
            source=HERE/'.work/source'/entry['path']
            assert hashlib.sha256(source.read_bytes()).hexdigest()==entry['sha256'], 'Extracted source changed: '+entry['path']
        print('Verified immutable pins and '+str(len(value['files']))+' extracted source digests; no runtime execution claimed.')
    else:
        print('Verified immutable pins; source not yet prepared; no runtime execution claimed.')
    return 0


if __name__=='__main__':
    try:
        sys.exit(main())
    except (OSError,ValueError,KeyError,AssertionError) as error:
        print('Reference verification failed: '+str(error),file=sys.stderr)
        sys.exit(1)
