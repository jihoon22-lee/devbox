#!/usr/bin/env python3
"""Validate the deliberately small, non-executable release invocation policy."""
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
POLICY = '.agents/skills/devbox-release/agents/openai.yaml'


def validate(text: str) -> None:
    lines = [line.rstrip() for line in text.splitlines()
             if line.strip() and not line.lstrip().startswith('#')]
    if len(lines) != 2 or lines[0] != 'policy:' or not lines[1].startswith('  '):
        raise ValueError('release policy must contain only the policy mapping')
    if lines[1] != '  allow_implicit_invocation: false':
        raise ValueError('devbox-release must remain explicitly invoked; unknown metadata needs review')


if __name__ == '__main__':
    try:
        validate((ROOT / POLICY).read_text(encoding='utf-8'))
    except (OSError, ValueError) as error:
        print(f'Agent metadata error: {error}', file=sys.stderr)
        sys.exit(1)
    print('Agent invocation metadata validated')
