#!/usr/bin/env bash
# 화면 용어 휴리스틱 검사. 생성물·테스트·import·주석은 제외한다.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
python3 - "$root" <<'PY'
from pathlib import Path
import re
import sys
source = Path(sys.argv[1]) / "app/src"
pattern = re.compile(r"Workbench|Code Pad|Repo Manager|Run Manager|Log Lens|Port Manager|⌘|provenance|receipt|generation|\broute\b")
interpolation = re.compile(r'(?<!\\)((?:\\\\)*)\$\{[A-Za-z_$][\w$]*\}')
bad = False
for path in sorted(source.rglob("*")):
    if path.suffix not in {".ts", ".tsx"} or "gen" in path.relative_to(source).parts or ".test." in path.name:
        continue
    for number, line in enumerate(path.read_text().splitlines(), 1):
        if line.lstrip().startswith(("//", "*", "import ", "export {")):
            continue
        # Check quoted text and JSX text without discarding whole router-related lines.
        strings = re.findall(r'''(["'`])((?:\\.|(?!\1).)*?)\1|>([^<]+)<''', line)
        # A plain interpolation names a variable, not displayed literal text.
        # Keep quoted expressions and escaped quotes visible to the word check.
        if any(pattern.search(interpolation.sub(r'\1', a) if quote == '`' else a or b)
               for quote, a, b in strings):
            print(f"{path}:{number}:{line}")
            bad = True
if bad:
    print("banned-words: 화면 문자열을 고쳐 주세요(docs/ui-terms.md).", file=sys.stderr)
    sys.exit(1)
print("banned-words: ok" if source.is_dir() else "banned-words: app/src 없음, 건너뜀")
PY
