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

print("dependency policy regression tests passed")
