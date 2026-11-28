#!/usr/bin/python3
import os, sys, signal
from pathlib import Path
root=Path(__file__).parent
(root/'pid').write_text(str(os.getpid()))
assert sys.argv[1:3] == ['logs', '--timestamps'], sys.argv
mode=sys.argv[-1]
assert mode in ['history','live','idle','eof','failure','tabs']
if mode in ['idle','live']:
    if mode == 'live': os.write(1,b'2026-01-01T00:00:01Z '+b'x'*1000000+b'\n')
    signal.pause()
elif mode == 'failure': sys.exit(1)
elif mode == 'tabs':
    os.write(1,b'2026-01-01T00:00:01Z hello\tworld\n2026-01-01T00:00:02Z hello\twide\tworld\n')
    os.write(2,b'2026-01-01T00:00:03Z final\n')
else:
    out=b'2026-01-01T00:00:01Z a\n2026-01-01T00:00:02Z b\n2026-01-01T00:00:03Z c\n2026-01-01T00:00:03Z d\n'
    err=b'2026-01-01T00:00:03Z e\n2026-01-01T00:00:04Z f\n'
    toggle=root/'toggle'
    if toggle.exists():
        os.write(2,err); os.write(1,out); toggle.unlink()
    else:
        os.write(1,out); os.write(2,err); toggle.touch()
