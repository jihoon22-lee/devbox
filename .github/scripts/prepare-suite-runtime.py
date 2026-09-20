"""Materialize verified candidate binaries in the established native fixture layout.

The target/debug directory is a fixture convention, not a debug rebuild. All
materialized product bytes come from the already verified release archives.
"""
import argparse
import json
from pathlib import Path
import shutil
import zipfile
from suite_release_contract import verify_public_assets


def prepare(assets, repository, source):
    manifest = verify_public_assets(assets, source=source)
    destination = repository/'target/debug'
    destination.mkdir(parents=True,exist_ok=True)
    portables = repository/'target/portable-fixture'
    portables.mkdir()
    for product in manifest['products']:
        with zipfile.ZipFile(assets/product['portable']['name']) as archive:
            portable = portables/product['id']
            portable.mkdir()
            for member in product['files']:
                target=portable/member['name']
                target.parent.mkdir(parents=True,exist_ok=True)
                with archive.open(member['name']) as src,target.open('xb') as dst:shutil.copyfileobj(src,dst,65536)
            names = [f"devbox-{product['id']}.exe"]
            if product['id']=='workspace':names += ['resources/wsl/manifest.json','resources/wsl/devbox-workspace-wsl']
            if product['id']=='control-center':names += ['resources/suite/devbox-suite-bootstrap.exe']
            for name in names:
                if name.startswith('resources/wsl/'):
                    target=repository/'apps/devbox-workspace/src-tauri'/name
                elif name.startswith('resources/suite/'):
                    target=destination/Path(name).name
                else:target=destination/name
                target.parent.mkdir(parents=True,exist_ok=True)
                if target.exists() and name!='resources/wsl/manifest.json':
                    raise ValueError('fixture output already exists')
                if target.is_symlink():raise ValueError('linked fixture destination')
                with archive.open(name) as src,target.open('wb') as dst:shutil.copyfileobj(src,dst,65536)
    print('Prepared exact candidate bytes for native fixture layout; no product build performed.')


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('assets',type=Path);parser.add_argument('--source',required=True)
    args=parser.parse_args();prepare(args.assets,Path.cwd(),args.source)
