#!/usr/bin/env python3
"""Write source-binding.json for the exact candidate (and, for the upgrade flows, baseline) SHA.

Pure: git show only, no host actions. The binding is bound by the approval's `binding` part
(sourceBindingSha256), so a new SHA needs only that small part re-reviewed (changes §1.3).
"""
import argparse
import json

from common import dump
from fixture import BINDING_FILE, bind_source


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--worktree', required=True)
    parser.add_argument('--candidate-sha', required=True)
    parser.add_argument('--baseline-sha')
    args = parser.parse_args()
    if BINDING_FILE.exists():
        raise SystemExit('source-binding.json exists; move it aside explicitly (prior binding is evidence)')
    binding = bind_source(args.worktree, args.candidate_sha, args.baseline_sha)
    dump(BINDING_FILE, binding)
    print(json.dumps(binding, indent=2))


if __name__ == '__main__':
    main()
