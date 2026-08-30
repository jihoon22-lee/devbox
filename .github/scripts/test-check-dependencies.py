#!/usr/bin/env python3
"""Focused regression tests for dependency policy expiry and lock parsing."""

import importlib.util
import sys
from datetime import date as real_date
from pathlib import Path


sys.dont_write_bytecode = True


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / ".github" / "scripts" / "check-dependencies.py"
spec = importlib.util.spec_from_file_location("check_dependencies", SCRIPT)
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)

policy = module.load_policy()
cargo_packages = module.cargo_lock_packages()
module.validate_exceptions(policy, cargo_packages)


class AfterExpiry(real_date):
    @classmethod
    def today(cls):
        return cls(2026, 11, 30)


module.date = AfterExpiry
try:
    module.validate_exceptions(policy, cargo_packages)
except SystemExit as error:
    assert "expired advisory exception" in str(error)
else:
    raise AssertionError("exceptions must fail closed on their expiry date")

module.date = real_date

integrities = module.parse_pnpm_integrities()
assert (
    integrities[("khroma", "2.1.0")]
    == "sha512-Ls993zuzfayK269Svk9hzpeGUKob/sIgZzyHYdjQoAdQetRKpOLj+k/QQQ/6Qi0Yz65mlROrfd+Ev+1+7dz9Kw=="
)

invalid_policy = dict(policy)
invalid_policy["licenseClarifications"] = [dict(policy["licenseClarifications"][0])]
invalid_policy["licenseClarifications"][0]["acceptedLicense"] = "Proprietary"
try:
    module.flatten_pnpm_licenses(
        {"Unknown": [{"name": "khroma", "versions": ["2.1.0"]}]},
        invalid_policy,
        integrities,
    )
except SystemExit as error:
    assert "outside the allowlist" in str(error)
else:
    raise AssertionError("clarifications must not bypass the license allowlist")

axe_integrity = integrities[("axe-core", "4.13.0")]
approved = module.flatten_pnpm_licenses(
    {"MPL-2.0": [{"name": "axe-core", "versions": ["4.13.0"]}]},
    {
        "allowedPnpmLicenses": [],
        "licenseClarifications": [],
        "packageLicenseApprovals": [dict(policy["packageLicenseApprovals"][0])],
    },
    {("axe-core", "4.13.0"): axe_integrity},
)
assert approved == [
    {
        "name": "axe-core",
        "version": "4.13.0",
        "license": "MPL-2.0",
        "source": "https://github.com/dequelabs/axe-core/tree/v4.13.0",
        "integrity": axe_integrity,
    }
]

try:
    module.flatten_pnpm_licenses(
        {"MPL-2.0": [{"name": "other-package", "versions": ["1.0.0"]}]},
        {
            "allowedPnpmLicenses": [],
            "licenseClarifications": [],
            "packageLicenseApprovals": [dict(policy["packageLicenseApprovals"][0])],
        },
        {("other-package", "1.0.0"): "sha512-other"},
    )
except SystemExit as error:
    assert "unapproved pnpm license expression" in str(error)
else:
    raise AssertionError("an exact package approval must not allow another MPL package")

bad_approval_policy = {
    "allowedPnpmLicenses": [],
    "licenseClarifications": [],
    "packageLicenseApprovals": [dict(policy["packageLicenseApprovals"][0])],
}
bad_approval_policy["packageLicenseApprovals"][0]["integrity"] = "sha512-wrong"
try:
    module.flatten_pnpm_licenses(
        {"MPL-2.0": [{"name": "axe-core", "versions": ["4.13.0"]}]},
        bad_approval_policy,
        {("axe-core", "4.13.0"): axe_integrity},
    )
except SystemExit as error:
    assert "approval integrity mismatch" in str(error)
else:
    raise AssertionError("a package approval must stay bound to its locked integrity")

print("dependency policy regression tests passed")
