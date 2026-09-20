#!/usr/bin/env python3
"""Synthetic package corruption and public-to-private byte identity regressions."""
import copy
import importlib.util
import json
from pathlib import Path
import shutil
import tempfile
import unittest
import zipfile
from suite_release_contract import PRODUCTS, file_names, manifest_assets, verify_public_assets, acceptance_config
SCRIPTS = Path(__file__).resolve().parent
SOURCE = 'a' * 40

def module(name):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / (name + '.py'))
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result

BUILDER = module('build-suite-package')
PREPARE = module('prepare-suite-fixture')
RUNTIME = module('prepare-suite-runtime')

def fixture(root):
    payload = root / 'input'
    payload.mkdir()
    (payload / 'THIRD_PARTY_NOTICES.md').write_bytes(b'synthetic license fixture\n')
    for product in PRODUCTS:
        for name in file_names(product) - {'THIRD_PARTY_NOTICES.md', 'devbox-installation.json'}:
            path = payload / product / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes((product + '/' + name).encode())
    assembly = root / 'assembly'
    BUILDER.portables(payload, assembly, '0.8.0', SOURCE)
    (assembly / 'Devbox_0.8.0_x64-setup.exe').write_bytes(b'synthetic installer, never executed')
    BUILDER.manifest(assembly, assembly / 'release-manifest.json')
    private = (assembly / 'suite-payload.json').read_bytes()
    (assembly / 'suite-payload.json').unlink()
    return assembly, private

class SuiteContractTests(unittest.TestCase):
    def test_public_roundtrip_recovers_exact_installer_payload(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            assets, original = fixture(root)
            manifest = verify_public_assets(assets, SOURCE, 'v0.8.0')
            self.assertEqual(len(list(assets.iterdir())), 7)
            PREPARE.prepare(assets, root / 'delivery', SOURCE, '123')
            self.assertEqual((root / 'delivery/suite-payload.json').read_bytes(), original)
            RUNTIME.prepare(assets, root / 'repo', SOURCE)
            for product in PRODUCTS:
                self.assertEqual({p.relative_to(root / 'repo/target/portable-fixture' / product).as_posix() for p in (root / 'repo/target/portable-fixture' / product).rglob('*') if p.is_file()}, file_names(product))
            self.assertEqual(len(acceptance_config(manifest)['products']), 4)
            with self.assertRaises(ValueError): verify_public_assets(assets, 'b'*40)
            with self.assertRaises(ValueError): verify_public_assets(assets, SOURCE, 'v0.8.1')

    def test_refuses_unknown_missing_and_tampered_assets_before_materialization(self):
        for mutation in ('extra', 'missing', 'tampered', 'linked'):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary); assets, _ = fixture(root)
                setup = assets / 'Devbox_0.8.0_x64-setup.exe'
                if mutation == 'extra': (assets / 'unknown.bin').write_bytes(b'x')
                if mutation == 'missing': setup.unlink()
                if mutation == 'tampered': setup.write_bytes(b'changed')
                if mutation == 'linked':
                    external = root / 'outside'; setup.rename(external); setup.symlink_to(external)
                with self.assertRaises(ValueError): PREPARE.prepare(assets, root / 'out', SOURCE, '123')
                self.assertFalse((root / 'out').exists())

    def test_rejects_inner_archive_corruption_even_with_updated_outer_digest(self):
        for mutation in ('traversal', 'duplicate', 'member-digest', 'missing-helper', 'symlink'):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary); assets, _ = fixture(root)
                path = assets / 'release-manifest.json'; manifest = json.loads(path.read_text())
                product = manifest['products'][0]; archive = assets / product['portable']['name']
                with zipfile.ZipFile(archive) as source: members = [(i, source.read(i)) for i in source.infolist()]
                with zipfile.ZipFile(archive, 'w') as target:
                    for info, data in members:
                        if mutation == 'missing-helper' and info.filename.endswith('devbox-workspace-wsl'): continue
                        if mutation == 'member-digest' and info.filename.endswith('.exe'): data = b'x'*len(data)
                        if mutation == 'symlink' and info.filename.endswith('.exe'): info.external_attr = 0o120777 << 16
                        target.writestr(info, data)
                    if mutation == 'traversal': target.writestr('../escape', b'x')
                    if mutation == 'duplicate': target.writestr('DEVBOX-WORKSPACE.EXE', b'x')
                product['portable'] = BUILDER.asset(archive)
                path.write_text(json.dumps(manifest))
                with self.assertRaises(ValueError): verify_public_assets(assets, SOURCE)

    def test_closed_product_and_component_manifest(self):
        with tempfile.TemporaryDirectory() as temporary:
            assets, _ = fixture(Path(temporary)); original = json.loads((assets / 'release-manifest.json').read_text())
            variants = []
            bad = copy.deepcopy(original); bad['products'].pop(); variants.append(bad)
            bad = copy.deepcopy(original); bad['products'][1] = bad['products'][0]; variants.append(bad)
            bad = copy.deepcopy(original); bad['products'][0]['files'].pop(1); variants.append(bad)
            bad = copy.deepcopy(original); bad['products'][0]['version'] = '0.7.0'; variants.append(bad)
            bad = copy.deepcopy(original); bad['setup']['size'] = True; variants.append(bad)
            for bad in variants:
                with self.assertRaises(ValueError): manifest_assets(bad)

if __name__ == '__main__': unittest.main()
