#!/usr/bin/env python3
"""SC8 source statistics. Lexically remove test-only Rust items, wherever placed."""
from pathlib import Path
import re
import subprocess
import sys


LITERAL = re.compile(r'''//|/\*|(?:br|r)\#*"|b?"|b?'(?:\\u\{[0-9a-fA-F_]+\}|\\.|[^\\'])' ''', re.X)


def masked_rust(text: str) -> str:
    """Keep code offsets/newlines; hide strings, chars and nested comments."""
    result = list(text)
    cursor = 0
    while match := LITERAL.search(text, cursor):
        start = match.start()
        token = match.group()
        if token == '//':
            end = text.find('\n', match.end())
            end = len(text) if end < 0 else end
        elif token == '/*':
            end, depth = match.end(), 1
            while depth and end < len(text):
                if text.startswith('/*', end):
                    depth += 1
                    end += 2
                elif text.startswith('*/', end):
                    depth -= 1
                    end += 2
                else:
                    end += 1
        elif re.match(r'(?:br|r)\#*"', token):
            closing = '"' + '#' * token.count('#')
            at = text.find(closing, match.end())
            end = len(text) if at < 0 else at + len(closing)
        elif token.endswith('"'):
            end = match.end()
            while end < len(text):
                if text[end] == '\\':
                    end += 2
                elif text[end] == '"':
                    end += 1
                    break
                else:
                    end += 1
        else:
            end = match.end()  # Character literal, not a Rust lifetime.
        for index in range(start, min(end, len(text))):
            if text[index] != '\n':
                result[index] = ' '
        cursor = end
    return ''.join(result)


def balanced_end(code: str, start: int) -> int:
    pairs = {'[': ']', '(': ')', '{': '}'}
    stack = []
    for index in range(start, len(code)):
        char = code[index]
        if char in pairs:
            stack.append(pairs[char])
        elif stack and char == stack[-1]:
            stack.pop()
            if not stack:
                return index + 1
    raise ValueError('Unbalanced Rust item or attribute')


def production_cfg(expression: str) -> bool | None:
    """Three-valued cfg: test=false; platform/feature flags unknown."""
    expression = expression.strip()
    if expression == 'test':
        return False
    match = re.fullmatch(r'(all|any|not)\s*\((.*)\)', expression, re.S)
    if not match:
        return None
    parts, start, depth = [], 0, 0
    for index, char in enumerate(match[2]):
        depth += (char == '(') - (char == ')')
        if char == ',' and depth == 0:
            parts.append(match[2][start:index])
            start = index + 1
    parts.append(match[2][start:])
    values = [production_cfg(part) for part in parts if part.strip()]
    if match[1] == 'not':
        return None if len(values) != 1 or values[0] is None else not values[0]
    if match[1] == 'all':
        return False if False in values else None if None in values else True
    return True if True in values else None if None in values else False


def without_rust_tests(text: str) -> str:
    code = masked_rust(text)
    removed, attributes = [], []
    for match in re.finditer(r'#\s*\[', code):
        if removed and match.start() < removed[-1][1]:
            continue
        end = balanced_end(code, match.end() - 1)
        body = code[match.end():end - 1].strip()
        cfg = re.fullmatch(r'cfg\s*\((.*)\)', body, re.S)
        test_only = body == 'test' or (cfg and production_cfg(cfg[1]) is False)
        if not test_only:
            attributes.append((match.start(), end))
            continue
        start = match.start()
        # Include other outer attributes belonging to this same item.
        for previous_start, previous_end in reversed(attributes):
            if code[previous_end:start].strip():
                break
            start = previous_start
        cursor = end
        while cursor < len(code):
            char = code[cursor]
            if char in '[(':
                cursor = balanced_end(code, cursor)
            elif char == '{':
                cursor = balanced_end(code, cursor)
                break
            elif char == ';':
                cursor += 1
                break
            else:
                cursor += 1
        removed.append((start, cursor))
    for start, end in reversed(removed):
        text = text[:start] + text[end:]
    return text


def counts(root: Path, files: list[str]) -> tuple[dict[str, int], list[dict]]:
    totals, large = {'rust': 0, 'ts': 0}, []
    for name in files:
        path = Path(name)
        if not (root / path).is_file() or any(p in {'tests', 'benches', 'gen', 'e2e', 'acceptance'} for p in path.parts):
            continue
        if re.search(r'(?:\.test\.|_tests?\.rs$)', name) or name == 'app/src/test-setup.ts':
            continue
        language = ('rust' if path.suffix == '.rs' and name.startswith(('crates/', 'app/src-tauri/src/', 'xtask/'))
                    else 'ts' if path.suffix in {'.ts', '.tsx'} and name.startswith('app/src/') else None)
        if language is None:
            continue
        text = (root / path).read_text()
        if language == 'rust':
            text = without_rust_tests(text)
        lines = len(text.splitlines())
        totals[language] += lines
        if lines > 600:
            large.append({'file': name, 'lines': lines})
    return totals, large


def main():
    root = Path(__file__).resolve().parent.parent
    budget = int(sys.argv[1]) if len(sys.argv) > 1 else 60000
    files = subprocess.check_output(['git', '-C', str(root), 'ls-files', '-z'], text=True).split('\0')
    totals, large = counts(root, files)
    total = sum(totals.values())
    print(f'rust={totals["rust"]} ts={totals["ts"]} total={total} budget={budget}')
    print(f'over_budget={total > budget} product_files_over_600={large}')


if __name__ == '__main__':
    main()
