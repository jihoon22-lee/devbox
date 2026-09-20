"""Reconstruct private test inputs from independently verified public Suite bytes."""
import argparse
import json
import os
from pathlib import Path
import shutil
import zipfile
from suite_release_contract import PRODUCTS, manifest_assets, verify_public_assets


def prepare(assets, output, source, run_id):
    manifest = verify_public_assets(assets, source=source)
    expected = manifest_assets(manifest, commit=source)
    output.mkdir()
    for name in set(expected) | {'release-manifest.json'}:
        shutil.copyfile(assets / name, output / name)
    payload = {"schemaVersion": 1, "suiteVersion": manifest['suiteVersion'], "sourceSha": source, "protocolVersion": 1, "products": manifest['products'], "notices": manifest['notices']}
    (output / 'suite-payload.json').write_text(json.dumps(payload, ensure_ascii=False, indent=2)+'\n', encoding='utf-8', newline='\n')
    center = next(p for p in manifest['products'] if p['id'] == 'control-center')
    with zipfile.ZipFile(assets / center['portable']['name']) as archive:
        with archive.open('resources/suite/devbox-suite-bootstrap.exe') as src, (output / 'devbox-suite-bootstrap.exe').open('xb') as dst:
            shutil.copyfileobj(src, dst, 65536)
    receipt = {'sourceSha':source,'runHeadSha':source,'repository':os.environ.get('GITHUB_REPOSITORY'),'runId':str(run_id),'productSources':{p:source for p in PRODUCTS},'scope':'exact-main-public-suite-fixture'}
    (output / 'suite-fixture-source.json').write_text(json.dumps(receipt, indent=2)+'\n', encoding='utf-8')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('assets',type=Path); parser.add_argument('output',type=Path)
    parser.add_argument('--source',required=True); parser.add_argument('--run-id',required=True)
    args=parser.parse_args(); prepare(args.assets,args.output,args.source,args.run_id)
