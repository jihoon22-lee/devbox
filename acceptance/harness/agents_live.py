"""S7 real-tool rows (changes §8.1): owned accept tasks with real tools, the owned tmux server and MCP.

Safety primitives (pure, unit-tested): the fixed texts (§8.1.1), TurnBudget, Pane with the Enter guard,
stop patterns and redaction (§8.1.2, §8.1.6), and trust_probe (§8.1.4). The row code below only calls
them; it never builds a key, a text or a tmux argument of its own. Nothing here imports a native helper.
"""
import bisect
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import stat
import subprocess
import time

from common import (EMAIL, REDACT_LINE, CommandFailed, dump, ensure_trust, error_evidence, redact as redact_lines, redact_json, redact_text,
                    require_lock, run_checked, trust_context)

TIMEOUT_TEXT = re.compile(r'timed out|시한을 초과')  # the same rule as evidence.TIMEOUT_TEXT (changes §1.6)

# --- 8.1.1 Fixed texts. `{nonce}` is the run nonce. -------------------------------------------------
PROMPT_TEMPLATES = {
    'P-OK': 'Reply with OK and stop.',
    'P-APPROVE': 'Run the shell command `touch s7-approved` in the current folder, then reply OK.',
    'P-APPROVE-X': 'Run the shell command `touch /tmp/devbox-s7-p1x-{nonce}` (it is outside this folder), then reply OK.',
    'P-ASK': 'Before doing anything, ask me exactly one clarifying question with your question tool, then wait.',
    'P-ENV': "Use your bash tool to run `env | grep '^DEVBOX_'`, then reply OK.",
    'P-NEST': 'Use your bash tool to run `opencode run "Reply with OK."`, then reply OK.',
}
SEND_EXTRA = {'exit': 'exit', 'quit': 'quit', ':q': ':q', 'T-FF': 's7 line\u000cend', 'T-ANSI': '\u001b[31ms7 red\u001b[0m',
              'T-ZW1': '\u200b!echo devbox-ok', 'T-ZW2': '\u2060!echo devbox-ok'}
NONCE = re.compile('[0-9a-f]{12}')


def prompt_texts(nonce):
    if not NONCE.fullmatch(str(nonce)):
        raise ValueError('Run nonce required')
    return {key: text.replace('{nonce}', nonce) for key, text in PROMPT_TEMPLATES.items()}


def send_texts(nonce):
    """SEND_TEXTS: agents.send and MCP send only these."""
    return {*prompt_texts(nonce).values(), *SEND_EXTRA.values()}


def text_for(nonce, key):
    texts = {**prompt_texts(nonce), **SEND_EXTRA}
    if key not in texts:
        raise ValueError('Unknown fixed text id: ' + str(key))
    return texts[key]


# Typed with `send-keys -l --` only. CL3's refusal command is checked separately, byte for byte.
KEY_TEXTS = ('! echo ok', '?', '!', '!echo s7', "!env | grep '^DEVBOX_'", '/exit', '/quit', '/feedback', '/rename', '/statusline',
             '/experimental', '/keymap', '/resume', '/side', '/goal Reply with OK')
KEYS = ('Enter', 'Escape', 'C-u', 'C-t', 'Up', 'Down')
# M8: the KEY_TEXTS that start a model turn once submitted; each counts against the tool's turn cap when typed.
TYPED_PROMPTS = ('/goal Reply with OK',)
EXIT_COMMANDS = {'claude': '/exit', 'codex': '/quit', 'opencode': '/exit', 'pi': '/quit'}
TOOLS = ('claude', 'codex', 'opencode', 'pi')
BUDGET = {'claude': 10, 'codex': 14, 'opencode': 12, 'pi': 5}
NESTED_BUDGET = {'opencode': 1}
TIMEOUTS = {'start': 60, 'turn': 180, 'screen': 30, 'attention': 2, 'attentionBound': 10, 'resume': 60, 'nested': 240, 'endLine': 10,
            'u1Turn': 120,
            # Ruling 86, the output-detected waits: the product's "2 to about 6 s" (crates/agents/src/watch.rs:56-57; a third tick, :111-116, :177, :191)
            # plus 1 s for the screen to settle, POLL and the RPC. The trust rows, H2X-RESUME (b), H2X-SCREENS and P1X (a) use it.
            'trustAttention': 7.0,
            # D7 (Ruling 153): claude 2.1.318 redraws the trust screen after the single Down (`❯ No, exit` is first); the Enter guard needs the
            # selection on `Yes, I trust this folder`, so accept_trust polls for it up to this bound and never presses Down twice.
            'trustSelect': 3.0}
BLOCK_CAPS = {'claude': 1800, 'codex': 2700, 'opencode': 1800, 'pi': 900}
POLL = 0.1
# D8 (Ruling 153): codex 0.167.0 takes a typed command followed 3-4 ms later by Enter as a paste and leaves it in the composer. Live.command
# waits for the typed text to be echoed (at most COMMAND_ECHO), settles COMMAND_SETTLE, re-captures, then presses Enter; for codex it then
# waits at most COMMAND_ECHO for the composer line to drop the text, else clears it with C-u.
COMMAND_SETTLE = 0.5
COMMAND_ECHO = 2.0
# 1.0.1 (D2): codex 0.172.0 lost the first character of text typed 4-8 ms after Escape (`/quit` arrived as `quit`, and a later `/quit`
# on top of it was submitted as the prompt `quit/quit`). After any key or paste, the next text, key or paste to the same pane first waits
# until KEY_SETTLE has passed and re-captures until two captures in a row are equal, at most KEY_SETTLE_MAX after the key (Pane.settle).
# Live.press settles right after a key that is not Enter, so a product send that follows a key is covered too.
KEY_SETTLE = 0.6
KEY_SETTLE_MAX = 3.0
# The Enter of rule (i) needs a fresh capture whose input row is exactly the typed command (input_echo). The input row is the last
# captured line that starts with the tool's prompt marker; the text anywhere else on the screen no longer counts. A tool without a
# known marker (opencode, pi: no capture of their input row exists yet) uses the stricter whole-line rule of input_echo.
INPUT_MARKERS = {'claude': ('❯',), 'codex': ('›',)}
SELECTORS = '›❯>→▶*'  # what a list may put in front of its selected row
SHELL_ECHO = 10.0  # CL3: how long the owned shell terminal may take to show the typed command
END_LINE = '[devbox] 에이전트가 끝났습니다(종료 코드'

# --- 8.1.2 Pane. -----------------------------------------------------------------------------------
TMUX = ('/usr/bin/tmux', '-L', 'devbox-accept')
TARGET_ID = re.compile('[A-Za-z0-9_-]+')
# The third format (1.0.1, UP1) identifies one tmux session and its pane across an app update.
DISPLAY_FORMATS = ('#{version}', '#{pane_width}x#{pane_height}', '#{session_created} #{pane_pid} #{pid}')
SIZES = ((200, 50), (40, 24), (80, 24))
PASTE_TEXTS = ('!', 's7-paste-probe')
TMUX_READ = ('capture-pane', 'has-session', 'list-buffers', 'display-message', 'show-options')
TMUX_MUTATE = ('send-keys', 'set-buffer', 'paste-buffer', 'resize-window', 'delete-buffer')
CL3_COMMAND = re.compile(r"git worktree remove --force '(/[A-Za-z0-9/._-]+)'")
STOP_PATTERNS = (('update', re.compile(r'(?i)update available|new version|update now')),
                 ('login', re.compile(r'/login|(?i:\bsign in\b|\blog in\b)')),
                 ('terms', re.compile(r'(?i)terms of (?:service|use)|privacy policy|accept (?:the )?terms')),
                 ('onboarding', re.compile(r'(?i)choose the text style|select login method|onboarding')),
                 ('api-key', re.compile(r'(?i)\bapi key\b')))
TOOL_STOP = {'pi': (('pi-trust', re.compile(r'Trust project folder\?')),)}
CLAUDE_TRUST_TEXT = ('Accessing workspace:', 'Yes, I trust this folder')
# The cursor (❯) on the trust choice; the accessible layout prefixes it with "y." (family_patterns.rs:50-58).
CLAUDE_TRUST_SELECTED = re.compile(r'(?m)^\s*❯\s*(?:y[.)]\s+)?Yes, I trust this folder\s*$')
# family_patterns.rs:139 (codex folder access) and the one choice that saves trust.
CODEX_TRUST_SCREEN = re.compile(r'(?im)^\s*(?:[›>]\s*)?(?:\d+[.)]\s+)?(?:Trust and continue|Open restricted|Open existing task)\s*$')
CODEX_TRUST_SELECTED = re.compile(r'(?m)^\s*›\s*(?:\d+[.)]\s+)?Trust and continue\s*$')
APPROVAL_COMMAND = 'touch s7-approved'
# Rule (iii), fix round 1 I2: the claude Bash permission block, header to options. The product has no pattern or
# fixture for this dialog (crates/agents/src/family_patterns.rs:47-58 covers only folder trust and shell mode; claude's
# hooks report the wait), so the shape is the harness's own: header, the command line, at most one description line,
# the question, then numbered options with exactly one cursor, on plain option 1.
APPROVAL_HEADER = 'Bash command'
APPROVAL_QUESTION = 'Do you want to proceed?'
APPROVAL_OPTION = re.compile(r'^(❯\s*)?(\d+)\.\s+(.*?)\s*$')
BOX_CHARS = '│┃╭╮╰╯─━┌┐└┘'
SHELL_META = re.compile(r'[;&|`$<>\\]')
APPROVAL_CASES = ('P1', 'U1X')
APPROVALS_PER_CASE = 2
# REDACT_LINE and EMAIL live in common since fix round 1 (the install, setup, doctor and MCP evidence use them too).
CAPTURE_LINES = 60
STOP_LINES = 15  # fix round 1 item 7: the screen a stop pattern fired on, kept in that case's evidence
# Round 2 item 1: a stop also keeps the captured lines that matched (at most STOP_MATCHES, each with STOP_CONTEXT lines
# above and below), and every recorded stop line is cut to STOP_WIDTH characters after redact().
STOP_MATCHES = 5
STOP_CONTEXT = 2
STOP_WIDTH = 400
# Round 2 item 2 (Ruling 83), a stop-pattern change: pi's passive update banner is the one allowed notice, for pi only.
# These three lines, in a row, each whole once spaces and box padding (BOX_CHARS) are stripped. A pi capture whose
# `update` matches all fall inside such banners does not stop; the banner goes to the case as `passiveNotices`. Any
# other update text stops, for every tool, and so does the banner when another line also matches `update`.
# Round 4 item 1 (Ruling 87): a capture is the last CAPTURE_LINES lines, so the banner can lose its first line above the
# top edge. Rows 0 and 1 that are exactly its last two lines are the banner too (pi_cut_banner), and its notice has
# `cut`. The bottom is the live screen and never cuts a banner.
PI_UPDATE_BANNER = (re.compile(r'Update Available'), re.compile(r'New version \d+\.\d+\.\d+ is available\. Run pi update'),
                    re.compile(r'Changelog: https://pi\.dev/changelog'))
# Fix round 3 (D10, Ruling 154): claude's login-expiry warning is a second allowed notice, for claude only: this exact line,
# whole once spaces and box padding are stripped. A claude capture whose `login` matches all fall on such lines does not
# stop on `login`; each goes to the case as `passiveNotices`. Any other `login` text in the capture still stops.
CLAUDE_LOGIN_NOTICE = re.compile(r'⚠ Your login expires in \d+ days? · run /login to renew')
NOTICE_LIMIT = 10  # distinct banners kept per case; more are only counted
# Round 2 item 5 (P-DEF-1): the owned start script, <accept dataDir>/agents/<id>/start.sh (crates/agents/src/prepare.rs:220).
ACCEPT_AGENTS = Path('.local/share/devbox/accept/agents')
AGENT_ID = re.compile('[A-Za-z0-9_-]{1,64}')
START_SCRIPT_LIMIT = 64 * 1024
END_CODE = re.compile(r'종료 코드 (-?\d+)\)')
EXIT_REASON = re.compile(r'exit:(-?\d+)')
ENDED_STATES = ('failed', 'review', 'closed')


class Refused(RuntimeError):
    """A harness guard refused a key, a text or a call; nothing was sent."""


class StopPrompt(RuntimeError):
    """An update, login, terms, onboarding or API-key screen: the case stops and records R-PROMPT.

    `pattern` is the stop pattern's name and `screen` the last STOP_LINES captured lines; `matches` holds the first
    STOP_MATCHES lines that matched it ({index, line, context}) and `match_count` how many lines matched. All of it
    went through redact() and is cut to STOP_WIDTH (round 2 item 1)."""

    def __init__(self, message, pattern=None, screen=None, matches=None, match_count=0):
        super().__init__(message)
        self.pattern, self.screen = pattern, list(screen or [])
        self.matches, self.match_count = list(matches or []), match_count


class BudgetExceeded(Refused):
    """The per-tool turn budget is used up."""


def redact(lines):
    return redact_lines(lines, CAPTURE_LINES)


def approval_dialog(lines):
    """The bottom-most permission block: {'content': lines between header and question, 'options': [...], 'cursors': ❯ lines
    from the header down}. Lines above the header (the transcript, the echoed prompt) are not part of it. None if absent."""
    rows = [line.strip().strip(BOX_CHARS).strip() for line in lines]
    header = max((i for i, row in enumerate(rows) if row == APPROVAL_HEADER), default=None)
    if header is None:
        return None
    question = next((i for i in range(header + 1, len(rows)) if rows[i] == APPROVAL_QUESTION), None)
    if question is None:
        return None
    content = [row for row in rows[header + 1:question] if row and not set(row) <= set(BOX_CHARS)]
    options, end = [], question + 1
    while end < len(rows) and APPROVAL_OPTION.match(rows[end]):
        match = APPROVAL_OPTION.match(rows[end])
        options.append({'number': int(match.group(2)), 'text': match.group(3), 'selected': bool(match.group(1))})
        end += 1
    cursors = sum(1 for row in rows[header:] if row.startswith('❯'))
    return {'content': content, 'options': options, 'cursors': cursors}


def approval_shown(lines):
    """Rule (iii): the block's command line is exactly APPROVAL_COMMAND, with at most one description line that holds
    no shell operator, and the one cursor from the header down is on the plain option 1 ("1. Yes"), never "don't ask
    again". The same predicate serves the Enter guard, approval_row and wait_approval."""
    block = approval_dialog(lines)
    if not block or not block['options']:
        return False
    content, options = block['content'], block['options']
    selected = [o for o in options if o['selected']]
    return (1 <= len(content) <= 2 and content[0] == APPROVAL_COMMAND and (len(content) == 1 or not SHELL_META.search(content[1]))
            and options[0]['number'] == 1 and block['cursors'] == 1 and len(selected) == 1
            and selected[0]['number'] == 1 and selected[0]['text'] == 'Yes')


def pattern_lines(pattern, lines):
    """The indexes of the lines a pattern matches in the joined capture, in order, once each. A match is mapped to the
    line it starts on, so a pattern that spans lines is still found (none of STOP_PATTERNS does)."""
    starts, offset = [], 0
    for line in lines:
        starts.append(offset)
        offset += len(line) + 1
    hits = []
    for match in pattern.finditer('\n'.join(lines)):
        index = bisect.bisect_right(starts, match.start()) - 1
        if not hits or hits[-1] != index:
            hits.append(index)
    return hits


def banner_rows(lines):
    """The lines as the pi banner rules compare them: surrounding spaces and box padding (BOX_CHARS) stripped."""
    return [line.strip().strip(BOX_CHARS).strip() for line in lines]


def pi_banners(lines):
    """The start indexes of PI_UPDATE_BANNER: its three lines in a row, each whole after spaces and BOX_CHARS are
    stripped (Ruling 83)."""
    rows = banner_rows(lines)
    return [i for i in range(len(rows) - 2) if all(p.fullmatch(rows[i + k]) for k, p in enumerate(PI_UPDATE_BANNER))]


def pi_cut_banner(lines):
    """Ruling 87: rows 0 and 1 are exactly PI_UPDATE_BANNER's last two lines, stripped as pi_banners strips them, so the
    banner's first line has scrolled above the capture. Only the top edge cuts; the capture ends at the live screen."""
    rows = banner_rows(lines[:2])
    return len(rows) == 2 and all(p.fullmatch(row) for p, row in zip(PI_UPDATE_BANNER[1:], rows))


def stop_check(lines, tool=None):
    """(pattern name or None, the matched line indexes, passive notices as (lines, cut)). For pi only, `update` matches
    that all fall inside pi's passive update banner, whole or cut at the top edge (pi_cut_banner, rows 0 and 1), are not
    a stop: those banners are returned as notices and the other patterns are still checked. The banner stops like any
    update text when another line also matches `update`. For claude only, `login` matches that all fall on
    CLAUDE_LOGIN_NOTICE lines (Ruling 154) are not a stop either: each such line is a one-line notice. Notices from both
    rules are added to the one list."""
    notices = []
    for name, pattern in STOP_PATTERNS + TOOL_STOP.get(tool, ()):
        hits = pattern_lines(pattern, lines)
        if not hits:
            continue
        if name == 'update' and tool == 'pi':
            starts = pi_banners(lines)
            cut = pi_cut_banner(lines)
            inside = {i + k for i in starts for k in range(len(PI_UPDATE_BANNER))} | ({0, 1} if cut else set())
            if set(hits) <= inside:
                notices += ([(lines[:2], True)] if cut else []) + [(lines[i:i + len(PI_UPDATE_BANNER)], False) for i in starts]
                continue
        if name == 'login' and tool == 'claude':
            rows = banner_rows(lines)
            found = [i for i, row in enumerate(rows) if CLAUDE_LOGIN_NOTICE.fullmatch(row)]
            if set(hits) <= set(found):
                notices += [(lines[i:i + 1], False) for i in found]
                continue
        return name, hits, notices
    return None, [], notices


def stop_reason(lines, tool=None):
    return stop_check(lines, tool)[0]


def echo_forms(text):
    """What an input row may show for a typed command: the text itself and, for a `!` command, the text without the `!`
    (a tool that switches to its shell mode takes the `!` as the mode key)."""
    forms = {text}
    if text.startswith('!') and text[1:].strip():
        forms.add(text[1:].strip())
    return forms


def input_row(lines, tool):
    """The text of the tool's input row: the last captured line that starts with its prompt marker (INPUT_MARKERS), with
    the marker, spaces and box padding stripped. None when the tool has no known marker or no line starts with it."""
    markers = INPUT_MARKERS.get(tool, ())
    for row in reversed(banner_rows(lines)):
        for marker in markers:
            if row.startswith(marker):
                return row[len(marker):].strip()
    return None


def plain_row(row, form):
    """A whole line that is the command, or a list row for it: an optional selector, the command, then nothing or spaces."""
    row = row[1:].strip() if row[:1] in SELECTORS else row
    return row == form or row.startswith(form + ' ')


def input_echo(lines, tool, text, before=None):
    """(shown, row): whether the capture shows `text` as exactly the tool's input row, and the row that was read (D2).

    `before` is the capture taken right before the text was typed; without it nothing passes. With a known prompt marker
    the input row is the last line that starts with it. It must equal the text now, and it must not have equalled it
    before the typing: a row that already showed the command (a transcript row that looks like the input row, a draft
    left there) is no proof that this typing arrived, so it never justifies an Enter. `quit/quit`, `quit` or a transcript
    line that holds the text never pass either. Without a marker, one more whole line must equal the text than in
    `before`, and no line may hold the text inside other text."""
    forms = echo_forms(text)
    if INPUT_MARKERS.get(tool):
        row = input_row(lines, tool)
        return before is not None and input_row(before, tool) not in forms and row is not None and row in forms, row
    rows = banner_rows(lines)
    exact = [row for row in rows if row in forms]
    holding = [row for row in rows if any(form in row for form in forms)]
    inside = [row for row in holding if not any(plain_row(row, form) for form in forms)]
    was = None if before is None else sum(1 for row in banner_rows(before) if row in forms)
    shown = was is not None and len(exact) == was + 1 and not inside
    return shown, (exact[-1] if exact else holding[-1] if holding else None)


def shell_echo(lines, command):
    """CL3's owned shell terminal: the last line that is not empty ends with the command and holds it once. Above it the
    command may stand at most once more, as a whole line: what the terminal echoed when the text arrived before the
    shell drew its prompt (the shell then shows the same input again after the prompt)."""
    rows = [line.rstrip() for line in lines if line.strip()]
    if not rows or not rows[-1].endswith(command) or rows[-1].count(command) != 1:
        return False
    above = [row for row in rows[:-1] if command in row]
    return len(above) <= 1 and all(row.strip() == command for row in above)


def shell_ready(previous, lines):
    """CL3: the shell has drawn its prompt and rests: two captures in a row are equal and their last line is not empty."""
    return previous is not None and lines == previous and any(line.strip() for line in lines)


def bounded(lines):
    """Stop evidence lines: through redact(), then each cut to STOP_WIDTH characters."""
    return [line[:STOP_WIDTH] for line in redact(lines)]


def stop_matches(lines, hits):
    """The first STOP_MATCHES matched lines, each with STOP_CONTEXT lines above and below (clamped at the edges)."""
    rows = []
    for index in hits[:STOP_MATCHES]:
        low, high = max(0, index - STOP_CONTEXT), min(len(lines), index + STOP_CONTEXT + 1)
        rows.append({'index': index, 'line': bounded([lines[index]])[0], 'context': bounded(lines[low:high])})
    return rows


def stop_evidence(error):
    """The case's `stop` and the driver's <case>-stop.json: the same bounded, redacted fields."""
    return {'pattern': error.pattern, 'lines': error.screen, 'matches': error.matches, 'matchCount': error.match_count}


class TurnBudget:
    """M8: one count per tool across every way a turn starts: agents.create and an accepted agents.send ('rpc'), an
    accepted MCP devbox_agent_send ('mcp') and a prompt typed through tmux (TYPED_PROMPTS, 'tmux'), against one cap per
    tool. A call is refused before it is made when no turn is left; `paths` only says how each turn was taken."""

    PATHS = ('rpc', 'mcp', 'tmux')

    def __init__(self, limits=None):
        self.limits = dict(limits or BUDGET)
        self.turns = {tool: 0 for tool in self.limits}
        self.paths = {path: {tool: 0 for tool in self.limits} for path in self.PATHS}
        self.nested = {tool: 0 for tool in self.limits}

    def check(self, tool, path='rpc'):
        if tool not in self.limits or path not in self.PATHS:
            raise BudgetExceeded('No turn budget for ' + str(tool))
        if self.turns[tool] >= self.limits[tool]:
            raise BudgetExceeded('Turn budget reached for %s (%d, all paths)' % (tool, self.limits[tool]))

    def took(self, tool, path='rpc'):
        self.turns[tool] += 1
        self.paths[path][tool] += 1

    def snapshot(self):
        return {'turns': dict(self.turns), 'paths': {p: dict(v) for p, v in self.paths.items()}, 'nested': dict(self.nested),
                'limits': dict(self.limits)}

    def nest(self, tool):
        if self.nested.get(tool, 0) >= NESTED_BUDGET.get(tool, 0):
            raise BudgetExceeded('Nested run budget reached for ' + str(tool))
        self.nested[tool] += 1


class Pane:
    """/usr/bin/tmux -L devbox-accept only, on recorded targets only (changes §8.1.2)."""

    def __init__(self, load, answers, nonce, run=subprocess.run, clock=time):
        self.load, self.answers, self.nonce, self.run, self.clock = load, answers, nonce, run, clock
        self.case = None
        self.seq = 0
        self.mutated = {}
        self.captured = {}
        self.last = {}
        self.trusted = {'claude': set(), 'codex': set()}
        self.approvals = {}
        self.buffers = []
        self.resized = set()
        self.stopped = None
        self.log = []
        # pi's passive update banners: {'case', 'lines', 'seen'} once per case and banner (Ruling 83), with 'cut' for one
        # cut at the capture's top edge (Ruling 87)
        self.notices = []
        self.notices_dropped = {}
        # D2: `keyed` holds the time of a key or paste the pane has not settled after; `tools` the tool of each pane's last
        # capture; `before` the capture a text was typed onto; `settles` one row per settle, for the case evidence.
        self.keyed, self.tools, self.before, self.settles = {}, {}, {}, []

    def begin(self, case):
        self.case = case

    def notice(self, lines, cut=False):
        kept = bounded(lines)
        for row in self.notices:
            if row['case'] == self.case and row['lines'] == kept:  # a cut banner's two lines never equal a whole one's three
                row['seen'] += 1
                return
        if sum(1 for row in self.notices if row['case'] == self.case) >= NOTICE_LIMIT:
            self.notices_dropped[self.case] = self.notices_dropped.get(self.case, 0) + 1
            return
        self.notices.append({'case': self.case, 'lines': kept, 'seen': 1, **({'cut': True} if cut else {})})

    def targets(self):
        m = self.load()
        ids = set(m.get('terminalIds', []))
        for agent in (m.get('agents') or {}).values():
            ids.update(agent.get('terminalIds', []))
        return ids

    def target(self, tid):
        if not isinstance(tid, str) or not TARGET_ID.fullmatch(tid) or tid not in self.targets():
            raise Refused('tmux target must be a recorded agent terminal or terminal.create id')
        return '=' + tid + ':'

    def _tmux(self, args, check=True):
        result = self.run([*TMUX, *args], capture_output=True, timeout=10)
        entry = {'t': round(self.clock.monotonic(), 3), 'case': self.case, 'cmd': args[0], 'exit': result.returncode}
        if result.returncode:
            entry['stderr'] = redact_text(result.stderr, 300)  # item 6: also for the unchecked queries; Live.finish keeps it
        self.log.append(entry)
        if check and result.returncode:
            raise CommandFailed(['tmux', args[0]], result.returncode, result.stderr)
        return result

    def capture(self, tid, tool=None):
        target = self.target(tid)
        out = self._tmux(['capture-pane', '-p', '-J', '-S', '-%d' % CAPTURE_LINES, '-t', target]).stdout
        lines = (out.decode('utf-8', 'replace') if isinstance(out, bytes) else out).splitlines()[-CAPTURE_LINES:]
        self.seq += 1
        self.captured[tid] = (self.seq, lines)
        if tool:
            self.tools[tid] = tool
        why, hits, notices = stop_check(lines, tool)
        for banner, cut in notices:
            self.notice(banner, cut)
        if why:
            self.stopped = why
            raise StopPrompt('Unexpected tool screen (%s); no further key is sent' % why, why, bounded(lines[-STOP_LINES:]),
                             stop_matches(lines, hits), len(hits))
        return lines

    def has_session(self, tid):
        return self._tmux(['has-session', '-t', self.target(tid)], check=False).returncode == 0

    def list_buffers(self):
        out = self._tmux(['list-buffers', '-F', '#{buffer_name}'], check=False).stdout
        return (out.decode() if isinstance(out, bytes) else out).split()

    def display(self, fmt, tid=None):
        if fmt not in DISPLAY_FORMATS:
            raise Refused('display-message format not allowed')
        args = ['display-message', '-p'] + (['-t', self.target(tid)] if tid else []) + [fmt]
        out = self._tmux(args).stdout
        return (out.decode() if isinstance(out, bytes) else out).strip()

    def show_extended_keys(self):
        out = self._tmux(['show-options', '-s', 'extended-keys'], check=False).stdout
        return (out.decode() if isinstance(out, bytes) else out).strip()

    def _mutate(self, tid, what, args):
        if self.stopped:
            raise Refused('Keys stopped after an unexpected tool screen (%s)' % self.stopped)
        self._tmux(args)
        self.seq += 1
        self.mutated[tid] = self.seq
        self.last[tid] = what
        if what[0] in ('key', 'paste'):
            self.keyed[tid] = self.clock.monotonic()

    def settle(self, tid, tool=None):
        """D2: after a key or a paste, wait until KEY_SETTLE has passed since it, then re-capture until two captures in a row
        are equal or KEY_SETTLE_MAX has passed. Returns the last capture, or None when no key is waiting. A stop screen
        raises as in capture(), and then no further key is sent."""
        sent = self.keyed.get(tid)
        if sent is None:
            return None
        tool = tool or self.tools.get(tid)
        wait = sent + KEY_SETTLE - self.clock.monotonic()
        if wait > 0:
            self.clock.sleep(wait)
        previous, stable = self.capture(tid, tool), False
        while not stable and self.clock.monotonic() < sent + KEY_SETTLE_MAX:
            self.clock.sleep(POLL)
            lines = self.capture(tid, tool)
            stable, previous = lines == previous, lines
        del self.keyed[tid]
        last = self.last.get(tid) or ('', '')
        self.settles.append({'case': self.case, 'after': last[1] if last[0] == 'key' else last[0],
                             'waited': round(self.clock.monotonic() - sent, 3), 'stable': stable})
        return previous

    def _settled(self, tid):
        """Before a text, a key or a paste: settle the pane after the key or paste that came before it."""
        if tid in self.keyed and not self.stopped:
            self.settle(tid)

    def type_text(self, tid, text, cl3_folder=None):
        target = self.target(tid)
        if text in KEY_TEXTS:
            what = ('text', text)
        else:
            match = CL3_COMMAND.fullmatch(text) if isinstance(text, str) else None
            if self.case != 'CL3' or not match or cl3_folder is None or match.group(1) != cl3_folder or cl3_folder not in self.load().get('agentWorktrees', []):
                raise Refused('Only KEY_TEXTS, or in CL3 the exact refusal command for the recorded owned folder')
            what = ('cl3', text)
        self._settled(tid)
        self.before[tid] = self.captured[tid][1] if self._fresh(tid) else None
        self._mutate(tid, what, ['send-keys', '-t', target, '-l', '--', text])

    def _fresh(self, tid):
        seen = self.captured.get(tid)
        return seen is not None and seen[0] > self.mutated.get(tid, -1)

    def enter_allowed(self, tid, rule, project=None):
        last = self.last.get(tid)
        if rule == 'command':
            # D2: the command's Enter needs a capture taken after the text was typed, whose input row is exactly the text.
            if not last or not self._fresh(tid):
                return False
            lines = self.captured[tid][1]
            if last[0] == 'cl3':
                return shell_echo(lines, last[1])
            return last[0] == 'text' and last[1][:1] in ('/', '!') and input_echo(lines, self.tools.get(tid), last[1], self.before.get(tid))[0]
        if not self._fresh(tid):
            return False
        text = '\n'.join(self.captured[tid][1])
        if rule == 'trust-claude':
            return (self.answers.get('Q1') is True and all(s in text for s in CLAUDE_TRUST_TEXT) and bool(CLAUDE_TRUST_SELECTED.search(text))
                    and project is not None and project not in self.trusted['claude'])
        if rule == 'trust-codex':
            selected = [line for line in text.splitlines() if line.lstrip().startswith('›')]
            return (self.answers.get('Q2') is True and 'Folder access' in text and bool(CODEX_TRUST_SCREEN.search(text))
                    and bool(CODEX_TRUST_SELECTED.search(text)) and not any('Open restricted' in s or 'Back' in s or 'Open existing' in s for s in selected)
                    and project is not None and project not in self.trusted['codex'])
        if rule == 'approve':
            return (self.case in APPROVAL_CASES and self.answers.get('Q5') is True and approval_shown(self.captured[tid][1])
                    and self.approvals.get(self.case, 0) < APPROVALS_PER_CASE)
        return False

    def key(self, tid, key, rule=None, project=None):
        target = self.target(tid)
        if key not in KEYS:
            raise Refused('Key not allowed: ' + str(key))
        # D2: the caller's own capture must be newer than the last text, key or paste, as before. The pane then settles after
        # a key or paste that came before, and the rule is checked on the newest capture.
        fresh = self._fresh(tid)
        self._settled(tid)
        if key == 'Enter':
            if not fresh or not self.enter_allowed(tid, rule, project):
                raise Refused('Enter refused: no rule (i)-(iii) holds for ' + str(rule))
            if rule in ('trust-claude', 'trust-codex'):
                self.trusted[rule.split('-')[1]].add(project)
            if rule == 'approve':
                self.approvals[self.case] = self.approvals.get(self.case, 0) + 1
        self._mutate(tid, ('key', key), ['send-keys', '-t', target, key])

    def paste(self, tid, text):
        target = self.target(tid)
        if text not in PASTE_TEXTS:
            raise Refused('Paste text not allowed')
        self._settled(tid)
        name = 's7-%s-%d' % (self.nonce, len(self.buffers) + 1)
        self.buffers.append(name)
        self._mutate(tid, ('paste', text), ['set-buffer', '-b', name, '--', text])
        self._mutate(tid, ('paste', text), ['paste-buffer', '-p', '-d', '-b', name, '-t', target])

    def resize(self, tid, width, height):
        target = self.target(tid)
        if (width, height) not in SIZES:
            raise Refused('Window size not allowed')
        self._mutate(tid, ('resize', (width, height)), ['resize-window', '-t', target, '-x', str(width), '-y', str(height)])
        if (width, height) == (80, 24):
            self.resized.discard(tid)
        else:
            self.resized.add(tid)

    def restore(self):
        """Resized windows back to 80x24; leftover s7-* buffers deleted (works after a stop too)."""
        done = []
        for tid in sorted(self.resized):
            try:
                self._tmux(['resize-window', '-t', self.target(tid), '-x', '80', '-y', '24'])
                done.append({'restored': tid})
            except Exception as error:
                done.append({'restore': tid, 'error': str(error)[:200]})
        self.resized.clear()
        prefix = 's7-%s-' % self.nonce
        for name in self.list_buffers():
            if name.startswith(prefix):
                self._tmux(['delete-buffer', '-b', name], check=False)
                done.append({'deleted': name})
        return done


# --- 8.1.4 trust_probe. ----------------------------------------------------------------------------
def trust_probe(home=None):
    """The only place tool-config paths appear (validate check). Read 'rb', parsed in memory; only a bool
    or a hash leaves. These files are never written, copied or printed."""
    home = Path(home) if home else Path.home()

    def claude_trusted(path):
        try:
            with open(home / '.claude.json', 'rb') as stream:
                projects = json.loads(stream.read()).get('projects') or {}
        except (FileNotFoundError, ValueError, AttributeError):
            return None
        entry = projects.get(str(path)) if isinstance(projects, dict) else None
        return None if not isinstance(entry, dict) else bool(entry.get('hasTrustDialogAccepted'))

    def codex_config_sha256():
        try:
            with open(home / '.codex/config.toml', 'rb') as stream:
                return hashlib.sha256(stream.read()).hexdigest()
        except FileNotFoundError:
            return None

    return claude_trusted, codex_config_sha256


# --- Row support. ----------------------------------------------------------------------------------
def tokens_total(tokens):
    return None if not isinstance(tokens, dict) else sum(int(tokens.get(k) or 0) for k in ('input', 'output', 'cacheRead', 'cacheWrite'))


def digest_lines(lines):
    return hashlib.sha256('\n'.join(lines).encode()).hexdigest()[:16]


def front_folder(line):
    """The folder of `export PATH='<dir>':"$PATH"` (crates/agents/src/start.rs:71), or None for any other shape."""
    prefix, suffix = 'export PATH=', ':"$PATH"'
    if not (line.startswith(prefix) and line.endswith(suffix)):
        return None
    try:
        words = shlex.split(line[len(prefix):-len(suffix)])
    except ValueError:
        return None
    return words[0] if len(words) == 1 else None


def start_script_binary(text):
    """Round 2 item 5, pure: from a start script's text only its `export PATH=` lines, the program its tool line (the
    line before `code=$?`, crates/agents/src/start.rs:125-132) runs, and the realpath of <PATH-front folder>/<program>
    when that program is a bare name. The shapes (start.rs:62-73): the folder in front with the tool by name, or a shared
    folder last with the tool as '<dir>/<name>'; either may run under `systemd-run ... --`. Nothing is executed."""
    lines = text.splitlines()
    path_lines = [line for line in lines if line.startswith('export PATH=')]
    program = None
    end = next((i for i, line in enumerate(lines) if line.strip() == 'code=$?'), None)
    if end:
        try:
            words = shlex.split(lines[end - 1])
        except ValueError:
            words = []
        if words[:1] == ['systemd-run']:
            words = words[words.index('--') + 1:] if '--' in words else []
        program = words[0] if words else None
    front = None
    for line in path_lines:
        front = front_folder(line) or front
    resolved = os.path.realpath(os.path.join(front, program)) if program and '/' not in program and front else None
    return {'pathLines': path_lines, 'program': program, 'resolved': resolved}


class RowFailed(RuntimeError):
    """A pass criterion that ran was contradicted."""


class Live:
    """One run's live context: owned RPC, manifest, pane, MCP and the per-case evidence."""

    def __init__(self, rpc, load, record, evidence, answers, nonce, pane, mcp, budget=None, clock=time, home=None, counter=None,
                 surface='wsl-artifact', windows_half=None):
        self.rpc, self.load, self.record_case, self.answers, self.nonce = rpc, load, record, answers, nonce
        self.pane, self.mcp, self.clock = pane, mcp, clock
        self.budget = budget or TurnBudget()
        self.home = Path(home) if home else Path.home()
        self.cases = Path(evidence) / 'cases'
        self.cases.mkdir(mode=0o700, exist_ok=True)
        self.data = {}
        # One index source per run for titles and branches (shared with daemon_rows).
        self.next_index = counter or iter(range(1, 500)).__next__
        self.sn2 = {}
        # 'wsl-artifact' in the WSL-only flows, 'installed-native' in the Windows flows; windows_half is
        # the R-IOP½ sentence a WSL-only run adds to the half-rows (U1, U1 추가).
        self.surface, self.windows_half = surface, windows_half
        self.claude_trusted, self.codex_config_sha256 = trust_probe(self.home)
        self.task_projects = {}  # agent id -> project id, for the discard preview at the end
        self.task_tools = {}  # agent id -> tool, for typed prompts (M8)
        self.task_cases = {}  # agent id -> the case that created it, which keeps its `toolBinary` (round 2 item 5)
        self.binaries = set()  # agent ids whose start script was read (once each)

    # evidence
    def note(self, case, key, value):
        self.data.setdefault(case, {}).setdefault(key, []).append(value)

    def put(self, case, key, value):
        self.data.setdefault(case, {})[key] = value

    def finish(self, case, status, reason=None, partial=None, failure=None, **details):
        row = self.data.get(case, {})
        # Item 6: the failed tmux commands of this case (the unchecked queries too), with exit code and redacted stderr.
        failed = [entry for entry in self.pane.log if entry.get('case') == case and entry['exit']][-20:]
        # Round 2 item 2: pi's passive update banners this case's captures showed, and how many captures showed each;
        # round 4 item 1: `cut` on a banner cut at the capture's top edge.
        notices = [{key: n[key] for key in ('lines', 'seen', 'cut') if key in n} for n in getattr(self.pane, 'notices', []) if n['case'] == case]
        dropped = getattr(self.pane, 'notices_dropped', {}).get(case)
        # D2: how often this case waited for the pane after a key, the longest wait, and the waits that ended without two equal captures.
        settled = [entry for entry in getattr(self.pane, 'settles', []) if entry.get('case') == case]
        if settled:
            row = {**row, 'keySettles': {'count': len(settled), 'maxWaitSeconds': max(entry['waited'] for entry in settled),
                                         'unstable': [entry for entry in settled if not entry['stable']][-20:]}}
        dump(self.cases / (case + '.json'), {'case': case, 'status': status, 'reason': reason, 'partial': partial, 'surface': self.surface, **row,
                                             **({'tmuxFailures': failed} if failed else {}), **({'passiveNotices': notices} if notices else {}),
                                             **({'passiveNoticesDropped': dropped} if dropped else {}), 'budget': self.budget.snapshot()})
        files = ['cases/' + case + '.json'] + (['cases/' + case + '-mcp.json'] if (self.cases / (case + '-mcp.json')).exists() else [])
        details.setdefault('surface', self.surface)
        self.record_case(case, status, files, reason, partial=partial, failure=failure, **details)

    # product calls
    def create(self, case, tool, project, prompt):
        m = self.load()
        project_id = (m.get('projectIdByName') or {}).get(project)
        if not project_id:
            project_id = self.rpc('projects.add', {'path': m['projectPaths'][project]})['id']
        self.budget.check(tool)
        index = self.next_index()
        worktree = True
        # Fix round 1 item 5: the app's create preview (NewTaskDialog.tsx:85-91), with the same worktree value, right before.
        self.note(case, 'trust', ensure_trust(self.rpc, trust_context(project_id, 'create', worktree=worktree)))
        params = {'projectId': project_id, 'title': 's7-%s-%d' % (tool.replace('s7-shell', 'shell'), index), 'profile': tool,
                  'worktree': worktree, 'prompt': text_for(self.nonce, prompt), 'branch': 's7-%s-%d' % (self.nonce, index)}
        task = self.rpc('agents.create', params)
        if tool in self.budget.limits:
            self.budget.took(tool)
        self.task_projects[task['id']] = project_id
        self.task_tools[task['id']] = tool
        self.task_cases[task['id']] = case
        self.note(case, 'creates', {'tool': tool, 'prompt': prompt, 'id': task['id'], 't': round(self.clock.monotonic(), 3)})
        return task

    def send(self, case, tool, agent_id, key):
        text = text_for(self.nonce, key)  # a non-fixed key is a harness error, never a recorded product refusal
        if tool in self.budget.limits:
            self.budget.check(tool)
        started = self.clock.monotonic()
        try:
            task = self.rpc('agents.send', {'id': agent_id, 'text': text})
            row = {'path': 'agents.send', 'text': key, 'result': 'accepted', 'state': task.get('state')}
            if tool in self.budget.limits:
                self.budget.took(tool)
        except Exception as error:
            row = {'path': 'agents.send', 'text': key, 'result': 'refused', 'code': getattr(error, 'code', None), 'message': str(error)[:300]}
        row['ms'] = round((self.clock.monotonic() - started) * 1000)
        self.note(case, 'sends', row)
        return row

    def mcp_send(self, case, tool, client, agent_id, key):
        if tool in self.budget.limits:
            self.budget.check(tool, 'mcp')
        result, text = client.send(agent_id, text_for(self.nonce, key))
        if result == 'accepted' and tool in self.budget.limits:
            self.budget.took(tool, 'mcp')
        row = {'path': 'mcp', 'text': key, 'result': result, 'reply': redact_text(text[:300], 300)}  # the first 300 characters, as before
        self.note(case, 'sends', row)
        self.mcp_evidence(case, client.pairs[-1:])
        return row

    def mcp_evidence(self, case, pairs):
        """cases/<ID>-mcp.json: this case's request/response pairs, at most 20 kB (changes §8.1.3)."""
        path = self.cases / (case + '-mcp.json')
        rows = json.loads(path.read_text()) if path.exists() else []
        rows += list(pairs)
        if len(json.dumps(rows, ensure_ascii=False)) <= 20000:
            dump(path, rows)

    def get(self, agent_id):
        task = self.rpc('agents.get', {'id': agent_id})
        if isinstance(task, dict) and task.get('terminalId') and agent_id not in self.binaries:
            self.read_tool_binary(agent_id)  # the first reply that names the terminal: the start script exists, the task runs
        return task

    def read_tool_binary(self, agent_id):
        """Round 2 item 5 (P-DEF-1 evidence): which binary a real-tool task ran, from its owned start script, read once
        and never executed. Only the `export PATH=` lines (redacted), the program the tool line runs and, for a bare
        name, the realpath of <PATH-front folder>/<program> go to the creating case as `toolBinary`. A read that fails
        is recorded as `error`; it never raises."""
        tool, case = self.task_tools.get(agent_id), self.task_cases.get(agent_id)
        if tool not in TOOLS or case is None or agent_id in self.binaries or not isinstance(agent_id, str) or not AGENT_ID.fullmatch(agent_id):
            return
        self.binaries.add(agent_id)
        entry = {'agentId': agent_id, 'tool': tool}
        try:
            path = self.home / ACCEPT_AGENTS / agent_id / 'start.sh'
            info = os.lstat(path)
            if not stat.S_ISREG(info.st_mode) or info.st_size > START_SCRIPT_LIMIT:
                raise ValueError('start.sh is not a regular file of at most %d bytes' % START_SCRIPT_LIMIT)
            descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
            with os.fdopen(descriptor, 'rb') as stream:
                data = stream.read(START_SCRIPT_LIMIT + 1)
            if len(data) > START_SCRIPT_LIMIT:
                raise ValueError('start.sh grew past %d bytes' % START_SCRIPT_LIMIT)
            parsed = start_script_binary(data.decode('utf-8', 'replace'))
            entry.update(program=bounded([parsed['program']])[0] if parsed['program'] else None, pathLines=bounded(parsed['pathLines'][-10:]),
                         resolved=bounded([parsed['resolved']])[0] if parsed['resolved'] else None)
        except Exception as error:
            entry['error'] = '%s: %s' % (type(error).__name__, redact_text(str(error), 300))
        self.note(case, 'toolBinary', entry)

    def listed(self, agent_id):
        for item in self.rpc('agents.list', {}).get('items') or []:
            if item.get('id') == agent_id:
                return item
        return None

    def terminal(self, agent_id, timeout=None):
        """The task's tmux terminal. agents.create replies `preparing` (crates/agents/src/create.rs:177) before the product
        names one, so a task still preparing is polled, bounded by TIMEOUTS['start']."""
        deadline = self.clock.monotonic() + (TIMEOUTS['start'] if timeout is None else timeout)
        while True:
            task = self.get(agent_id)
            if task.get('terminalId') or task.get('state') not in ('preparing', None) or self.clock.monotonic() >= deadline:
                return task.get('terminalId')
            self.clock.sleep(POLL * 5)

    def capture(self, case, tid, tool=None, keep=True):
        lines = self.pane.capture(tid, tool)
        if keep:
            self.note(case, 'captures', {'t': round(self.clock.monotonic(), 3), 'lines': redact(lines)})
        return lines

    def press(self, case, tid, key, rule=None, project=None):
        self.pane.key(tid, key, rule, project)
        self.note(case, 'keys', {'t': round(self.clock.monotonic(), 3), 'key': key, 'rule': rule})
        if key != 'Enter':
            # D2: nothing follows a key before the pane has settled, a product send (agents.send, MCP) included.
            self.settle(tid)

    def settle(self, tid, tool=None):
        """D2: Pane.settle for a key or paste this pane has not settled after; None when there is none."""
        settle = getattr(self.pane, 'settle', None)
        return settle(tid, tool) if settle else None

    def command(self, case, tid, text, tool):
        """Type a command and submit it once the tool's input row is exactly the text (D8, Ruling 153; D2, 1.0.1).

        The pane is settled after any earlier key and captured before the text is typed. Enter is pressed only after a
        capture, and COMMAND_SETTLE later a second one, show the text as exactly the input row (input_echo); the pane's own
        Enter rule (i) checks the same on that last capture. Returns False when the row never was the text (no Enter) or
        codex kept the text in its composer after Enter."""
        self.settle(tid, tool)
        before = self.capture(case, tid, tool)
        self.type(case, tid, text)
        deadline = self.clock.monotonic() + COMMAND_ECHO
        while True:
            lines = self.capture(case, tid, tool)
            shown, row = input_echo(lines, tool, text, before)
            if shown:
                break
            if self.clock.monotonic() >= deadline:
                return self.no_echo(case, tid, tool, text, before, row)
            self.clock.sleep(POLL)
        self.clock.sleep(COMMAND_SETTLE)
        shown, row = input_echo(self.capture(case, tid, tool), tool, text, before)
        if not shown:
            return self.no_echo(case, tid, tool, text, before, row)
        self.press(case, tid, 'Enter', 'command')
        if tool != 'codex':
            return True
        deadline = self.clock.monotonic() + COMMAND_ECHO
        while True:
            lines = self.capture(case, tid, tool)
            if input_row(lines, tool) != text:  # the composer is the last `›` row; a transcript row above it does not count
                return True
            if self.clock.monotonic() >= deadline:
                break
            self.clock.sleep(POLL)
        self.press(case, tid, 'C-u')
        self.note(case, 'leftover', text)
        return False

    def no_echo(self, case, tid, tool, text, before, row):
        """The input row is not exactly the command: no Enter is sent. What the typing left on a known input row is cleared
        with C-u, so the next command does not land on it; a screen without a known input row gets no key at all."""
        was = input_row(before, tool)
        self.note(case, 'noEcho', text)
        self.note(case, 'inputRows', {'text': text, 'before': None if was is None else bounded([was])[0],
                                      'after': None if row is None else bounded([row])[0]})
        if INPUT_MARKERS.get(tool) and row is not None and row != was:
            self.press(case, tid, 'C-u')
            self.note(case, 'cleared', text)
        return False

    def tool_of(self, tid):
        """The profile of the recorded owned task whose terminal this is, or None."""
        for agent_id, row in (self.load().get('agents') or {}).items():
            if tid in (row.get('terminalIds') or []):
                return self.task_tools.get(agent_id) or row.get('profile')
        return None

    def type(self, case, tid, text, cl3_folder=None):
        tool = None
        if text in TYPED_PROMPTS:
            tool = self.tool_of(tid)
            if tool not in self.budget.limits:
                raise Refused('A typed prompt needs the turn budget of a known tool; this terminal has none')
            self.budget.check(tool, 'tmux')
        self.pane.type_text(tid, text, cl3_folder)
        if tool:
            self.budget.took(tool, 'tmux')
        self.note(case, 'keys', {'t': round(self.clock.monotonic(), 3), 'text': text if text in KEY_TEXTS else 'CL3-command'})

    def watch(self, case, agent_id, until, timeout, tool=None, settle=1.5):
        """Poll agents.get, agents.list and the pane every 100 ms until a state in `until`, then settle."""
        rows, start, done_at = [], self.clock.monotonic(), None
        deadline = start + timeout
        task = None
        while True:
            tick = self.clock.monotonic()
            task = self.get(agent_id)
            item = self.listed(agent_id)
            tid = task.get('terminalId')
            lines = self.pane.capture(tid, tool) if tid else []
            rows.append({'t': round(tick - start, 3), 'get': task.get('state'), 'list': (item or {}).get('state'),
                         'attention': bool(task.get('attention')), 'pane': digest_lines(lines), 'ok': any('OK' in line for line in lines)})
            if done_at is None and (item or {}).get('state') in until:
                done_at = tick
            if done_at is not None and tick - done_at >= settle:
                break
            if tick >= deadline:
                break
            self.clock.sleep(max(0.0, POLL - (self.clock.monotonic() - tick)))
        self.note(case, 'transitions', rows[-400:])
        if rows:
            self.note(case, 'captures', {'t': round(self.clock.monotonic(), 3), 'lines': redact(lines)})
        return rows, task

    def wait_pane(self, case, tid, predicate, timeout, tool=None):
        start = self.clock.monotonic()
        while True:
            lines = self.pane.capture(tid, tool)
            if predicate(lines):
                self.note(case, 'captures', {'t': round(self.clock.monotonic(), 3), 'lines': redact(lines)})
                return lines, round(self.clock.monotonic() - start, 3)
            if self.clock.monotonic() - start >= timeout:
                self.note(case, 'captures', {'t': round(self.clock.monotonic(), 3), 'lines': redact(lines)})
                return None, None
            self.clock.sleep(POLL)

    def end(self, case, agent_id):
        """End an owned task through the product as the app's discard (useReviewAction.ts:65-75): the preview with the
        task id, agents.discard_check, then agents.discard."""
        try:
            project_id = (self.task_projects.get(agent_id) or ((self.load().get('agents') or {}).get(agent_id) or {}).get('projectId')
                          or self.get(agent_id).get('projectId'))
            self.note(case, 'trust', ensure_trust(self.rpc, trust_context(project_id, 'discard', agent_id=agent_id)))
            check = self.rpc('agents.discard_check', {'id': agent_id})
            expect = {'headOid': check['headOid'], 'unmergedCommits': check['unmergedCommits']}
            if check.get('fingerprint') is not None:
                expect['discardFingerprint'] = check['fingerprint']
            self.rpc('agents.discard', {'id': agent_id, 'expect': expect})
            self.note(case, 'ended', {'id': agent_id, 'how': 'discard'})
        except Exception as error:
            self.note(case, 'ended', {'id': agent_id, 'error': str(error)[:300]})

    def doctor_rows(self):
        checks = self.rpc('system.doctor', {}).get('checks') or []
        return {c['id']: {'status': c.get('status'), 'message': str(c.get('message', ''))[:200]} for c in checks if str(c.get('id', '')).startswith('tool.')}


def idle_follow(rows):
    """Seconds from the first capture of the final pane content (with a reply) to Idle in agents.list."""
    idle = next((r for r in rows if r['list'] == 'idle'), None)
    if idle is None:
        return None
    final = rows[-1]['pane']
    first = next((r for r in rows if r['pane'] == final and r['ok']), None)
    return None if first is None else round(idle['t'] - first['t'], 3)


def went(rows, *states):
    seen = [r['get'] for r in rows]
    index = 0
    for state in seen:
        if index < len(states) and state == states[index]:
            index += 1
    return index == len(states)


def h_checks(live, case, tool, task, rows):
    """H1-H4 (a)-(d) for a task whose launch was watched. Returns {part: True|False|None(not run)}."""
    parts = {}
    delay = idle_follow(rows)
    live.put(case, 'idleFollowSeconds', delay)
    parts['a'] = went(rows, 'running', 'idle') and delay is not None and delay <= 2.0
    session = live.get(task['id']).get('sessionId')
    tid = live.terminal(task['id'])
    if not live.command(case, tid, EXIT_COMMANDS[tool], tool):
        parts['b'] = None
        live.put(case, 'resume', EXIT_COMMANDS[tool] + ' stayed in the composer')
    else:
        ended, waited = live.wait_pane(case, tid, lambda lines: any(END_LINE in line for line in lines), TIMEOUTS['endLine'], tool)
        live.put(case, 'endLineSeconds', waited)
        if ended is None:
            parts['b'] = None
            live.put(case, 'resume', 'end line did not appear within 10 s')
        else:
            resumed = live.rpc('agents.resume', {'id': task['id']})
            rows2, after = live.watch(case, task['id'], ('idle',), TIMEOUTS['resume'], tool)
            live.put(case, 'resume', {'sessionBefore': session, 'sessionAfter': resumed.get('sessionId'), 'state': after.get('state')})
            parts['b'] = resumed.get('sessionId') == session and after.get('state') == 'idle'
    usage = live.rpc('agents.usage', {'id': task['id']})
    item = live.listed(task['id']) or {}
    total, listed = tokens_total(usage.get('tokens')), tokens_total(item.get('tokens'))
    live.put(case, 'usage', {'usageTotal': total, 'listTotal': listed, 'truncated': usage.get('truncated')})
    parts['c'] = bool(total) and total > 0 and total == listed
    doctor = live.doctor_rows()
    live.put(case, 'doctor', doctor)
    parts['d'] = all(doctor.get('tool.' + t, {}).get('status') == 'ok' for t in TOOLS)
    bad = {k: v for k, v in doctor.items() if k in ['tool.' + t for t in TOOLS] and v.get('status') != 'ok'}
    if bad and len(bad) == len([k for k in bad if TIMEOUT_TEXT.search(bad[k].get('message', ''))]):
        # Changes §1.6: a tool.* row that reports the 2 s probe deadline is a shell-init stall candidate.
        first = sorted(bad)[0]
        live.put(case, 'failure', {'step': 'doctor:' + first, 'reason': 'system.doctor %s: %s' % (first, bad[first]['message']),
                                   'shellInit': 'doctor-tool', 'message': bad[first]['message'], 'timeout': True})
    return parts


def conclude(live, case, parts, not_run_reasons, record_only=False):
    """PASS only when every part ran and passed; FAIL if a part that ran failed; else NOT_RUN(부분: …)."""
    failed = [k for k, v in parts.items() if v is False]
    passed = [k for k, v in parts.items() if v is True]
    live.put(case, 'parts', parts)
    stall = live.data.get(case, {}).pop('failure', None)
    if failed:
        # Only a doctor-row stall that is the sole failing part is offered to the interop attribution.
        failure = stall if failed == ['d'] and stall else {'step': case, 'reason': 'parts ' + ','.join(failed)}
        live.finish(case, 'FAIL', 'Failed parts: ' + ', '.join(failed), failure=failure)
    elif not not_run_reasons and len(passed) == len(parts):
        live.finish(case, 'PASS', 'record-only observations recorded' if record_only else None)
    else:
        live.finish(case, 'NOT_RUN', ' '.join(not_run_reasons) or 'Parts not run: ' + ', '.join(k for k, v in parts.items() if v is None), partial=passed)


def waiting_checks(live, case, tool, agent_id, tid, client, text='P-OK', bound=None, since=None):
    """Attention waiting within the bound, agents.send and MCP refused with awaiting_input, pane input 0.

    Ruling 85: the bound is the caller's. A hook-detected wait keeps TIMEOUTS['attention'] (2 s) from this call; the
    folder-trust rows pass TIMEOUTS['trustAttention'] (7.0 s, Ruling 86) and `since`, the time of the first capture that
    showed the trust screen. codex's output-detected waits whose parts count `attention` (H2X-RESUME (b), H2X-SCREENS,
    P1X (a)) pass the same bound, from this call (R3-M2). The `checks` dict, every key and value, goes to the case as
    `checks` on every exit, and the bound as `attentionBoundSeconds` (round 2 item 3).

    Fix round 1 I1: the product's own detector is what these rows test, so the harness never relies on it alone.
    Nothing is sent unless `waiting` was observed; and right before each of the two sends a fresh agents.get must
    still say `waiting` and a fresh capture must show no stop screen. Otherwise that send and every later one are
    skipped and (b) is false; a stop screen records (b) false and ends the row (R-PROMPT). The gate rows are kept
    as `sendGate`. M11: a screen can still appear between that capture and the send; the re-check narrows it."""
    bound = TIMEOUTS['attention'] if bound is None else bound
    start = live.clock.monotonic() if since is None else since
    attention, state = None, None
    while live.clock.monotonic() - start <= max(TIMEOUTS['attentionBound'], bound):
        state = live.get(agent_id).get('state')
        if state == 'waiting':
            attention = round(live.clock.monotonic() - start, 3)
            break
        live.clock.sleep(POLL)
    live.put(case, 'attentionSeconds', attention)
    live.put(case, 'attentionBoundSeconds', bound)
    checks = {'attention': attention is not None and attention <= bound, 'send': False, 'mcp': False, 'mcpRefused': False,
              'input0': False}
    live.note(case, 'checks', checks)  # the same dict: what the returns below set is what the evidence keeps
    gates = []
    live.put(case, 'sendGate', gates)
    if attention is None:
        gates.append({'before': 'agents.send', 'state': state, 'sent': False, 'why': 'waiting was never observed'})
        return checks

    def gate(path):
        state = live.get(agent_id).get('state')
        try:
            lines = live.capture(case, tid, tool)
        except StopPrompt as error:
            gates.append({'before': path, 'state': state, 'sent': False, 'stop': error.pattern})
            live.put(case, 'parts', {'b': False})
            raise
        gates.append({'before': path, 'state': state, 'sent': state == 'waiting'})
        return state == 'waiting', lines
    buffers = live.pane.list_buffers()
    ok, before = gate('agents.send')
    if not ok:
        return checks
    sent = live.send(case, tool, agent_id, text)
    checks['send'] = sent['result'] == 'refused' and sent.get('code') == 'agents.awaiting_input'
    ok, _ = gate('mcp')
    if not ok:
        return checks
    mcp = live.mcp_send(case, tool, client, agent_id, text)
    after = live.capture(case, tid, tool)
    new_buffers = [b for b in live.pane.list_buffers() if b not in buffers]
    checks.update(mcp=mcp['result'] == 'refused' and mcp['reply'].startswith('에이전트가 선택을 기다리고 있습니다.'), mcpRefused=mcp['result'] == 'refused',
                  input0=before == after and not new_buffers)
    return checks


def sn2_part(live, tool, agent_id, client):
    """SN2, record only: per path and character, the result, how the pane shows it and the reply.

    M8: a send the tool's one turn cap refuses is not made. It is kept as `not sent` (code `turn-budget`) and the part
    stays partial in record_sn2(); the sends before it are kept. pi needs six turns here (create, K1X-SEND and these
    four) against pi 5, so its last MCP send is the one refused."""
    rows = []
    tid = live.terminal(agent_id)
    for path in ('agents.send', 'mcp'):
        for key in ('T-ZW1', 'T-ZW2'):
            try:
                row = live.send('SN2', tool, agent_id, key) if path == 'agents.send' else live.mcp_send('SN2', tool, client, agent_id, key)
            except BudgetExceeded as error:
                rows.append({'tool': tool, 'path': path, 'text': key, 'result': 'not sent', 'code': 'turn-budget', 'reason': str(error)[:200]})
                continue
            settled, _ = live.watch('SN2', agent_id, ('idle',), TIMEOUTS['turn'], tool)
            lines = live.capture('SN2', tid, tool)
            rows.append({'tool': tool, 'path': path, 'text': key, 'result': row['result'], 'code': row.get('code'),
                         'shellOutput': any(line.strip() == 'devbox-ok' for line in lines),
                         'asPrompt': any('!echo devbox-ok' in line for line in lines), 'stateAfter': settled[-1]['get'] if settled else None})
    live.sn2[tool] = rows


def run_block(live, tool, selected, steps):
    """Run a tool block's rows in order. A guard refusal, a stop screen or an error ends the block safely.

    SN2 is concluded once by record_sn2(); a block that ends early leaves its SN2 part unrecorded,
    unless the SN2 step itself raised (then SN2 is FAIL, which sticks).
    """
    if not selected:
        return
    require_lock()
    live.pane.stopped = None
    deadline = live.clock.monotonic() + BLOCK_CAPS[tool]
    pending = [case for case, _ in steps if case in selected]
    tasks = []
    current = None
    try:
        for case, step in steps:
            if case not in selected:
                continue
            if live.clock.monotonic() > deadline:
                raise Refused('Block time cap reached for ' + tool)
            current = case
            live.pane.begin(case)
            step(live, tasks)
            pending.remove(case)
            current = None
    except StopPrompt as error:
        if current is not None:
            # Fix round 1 item 7 and round 2 item 1: the pattern, the matched lines with their context and the last 15
            # captured lines (redacted, bounded), on the case whose capture stopped.
            live.put(current, 'stop', stop_evidence(error))
        reason = live_reason('R-PROMPT') + ' (' + str(error)[:120] + ')'
        for case in [c for c in pending if c != 'SN2']:
            live.finish(case, 'NOT_RUN', reason)
    except Refused as error:
        for case in [c for c in pending if c != 'SN2']:
            live.finish(case, 'NOT_RUN', 'Harness guard refused: ' + str(error)[:300])
    except Exception as error:
        for case in pending:
            if case == current:
                live.put(case, 'error', error_evidence(error))
                live.finish(case, 'FAIL', 'Row failed: %s: %s' % (type(error).__name__, str(error)[:500]),
                            failure={'step': case, 'reason': str(error)[:500], 'stderr': getattr(error, 'stderr', None)})
            elif case != 'SN2':
                live.finish(case, 'NOT_RUN', 'Not reached: an earlier row of the %s block failed' % tool)
    finally:
        for agent_id in tasks:
            live.end('cleanup-' + tool, agent_id)
        live.put('cleanup-' + tool, 'pane', live.pane.restore())


def live_reason(code):
    import evidence
    return evidence.reason(code)


def row_text(key):
    import evidence
    return evidence.ROW_TEXT[key]


# --- opencode block: H3, the H3 추가 rows, SN1, SN2 opencode part. ------------------------------------
def opencode_block(live, selected, client):
    state = {}

    def h3(live, tasks):
        task = live.create('H3', 'opencode', 'P-TOOLS', 'P-OK')
        tasks.append(task['id'])
        state['task'] = task
        rows, _ = live.watch('H3', task['id'], ('idle',), TIMEOUTS['turn'], 'opencode')
        state['launch'] = rows
        parts = h_checks(live, 'H3', 'opencode', task, rows)
        state['resume'] = live.data['H3'].get('transitions', [[]])[-1]
        conclude(live, 'H3', parts, [live_reason('R-TOAST')])

    def ensure(live, tasks, case):
        if 'task' not in state:
            task = live.create(case, 'opencode', 'P-TOOLS', 'P-OK')
            tasks.append(task['id'])
            state['task'] = task
            state['launch'], _ = live.watch(case, task['id'], ('idle',), TIMEOUTS['turn'], 'opencode')
        return state['task']

    def report(live, tasks):
        ensure(live, tasks, 'H3X-REPORT')
        launch, resume = state.get('launch') or [], state.get('resume') or []
        follow = idle_follow(launch)
        quiet = [r for r in resume if r['list'] == 'idle']
        changes = [i for i in range(1, len(quiet)) if quiet[i]['list'] != quiet[i - 1]['list'] and quiet[i]['pane'] == quiet[i - 1]['pane']]
        live.put('H3X-REPORT', 'launchFollowSeconds', follow)
        parts = {'launch': went(launch, 'running', 'idle') and follow is not None and follow <= 2.0,
                 'resume': (None if not resume else (not changes and bool(quiet)))}
        conclude(live, 'H3X-REPORT', parts, [] if resume else ['The resume timeline is from H3; H3 was not run'])

    def config(live, tasks):
        task = ensure(live, tasks, 'H3X-CONFIG')
        conclude(live, 'H3X-CONFIG', *opencode_config(live, task))

    def shell(live, tasks):
        task = ensure(live, tasks, 'H3X-SHELL')
        tid = live.terminal(task['id'])
        live.type('H3X-SHELL', tid, '!')
        lines = live.capture('H3X-SHELL', tid, 'opencode')
        hint = any('esc exit shell mode' in line for line in lines)
        checks = waiting_checks(live, 'H3X-SHELL', 'opencode', task['id'], tid, client)
        live.press('H3X-SHELL', tid, 'Escape')
        live.pane.paste(tid, '!')
        live.note('H3X-SHELL', 'keys', {'t': round(live.clock.monotonic(), 3), 'paste': '!'})
        pasted = live.capture('H3X-SHELL', tid, 'opencode')
        live.put('H3X-SHELL', 'pastedBangShellMode', any('esc exit shell mode' in line for line in pasted))
        live.press('H3X-SHELL', tid, 'Escape')
        parts = {'a': hint, 'b': checks['send'] and checks['mcpRefused'] and checks['input0'], 'c': True}
        conclude(live, 'H3X-SHELL', parts, [row_text('H3X-SHELL.retry')])

    def sn1(live, tasks):
        task = ensure(live, tasks, 'SN1')
        tid = live.terminal(task['id'])
        before = live.capture('SN1', tid, 'opencode')
        results = [live.send('SN1', 'opencode', task['id'], key) for key in ('T-FF', 'T-ANSI')]
        mcp = [live.mcp_send('SN1', 'opencode', client, task['id'], key) for key in ('T-FF', 'T-ANSI')]
        after = live.capture('SN1', tid, 'opencode')
        parts = {'a': all(r['result'] == 'refused' and r.get('code') == 'agents.control_characters' for r in results),
                 'b': all(r['result'] == 'refused' and r['reply'].startswith('보낼 글에 제어 문자가 있어') for r in mcp),
                 'c': before == after}
        conclude(live, 'SN1', parts, [live_reason('R-UI-PREFIX') + row_text('SN1.ui')])

    def sn2(live, tasks):
        task = ensure(live, tasks, 'SN2')
        sn2_part(live, 'opencode', task['id'], client)

    def exits(live, tasks):
        task = ensure(live, tasks, 'H3X-EXIT')
        rows = []
        for key in ('exit', 'quit', ':q'):
            settled, _ = live.watch('H3X-EXIT', task['id'], ('idle',), TIMEOUTS['turn'], 'opencode', settle=0.2)
            sent = live.send('H3X-EXIT', 'opencode', task['id'], key)
            tid = live.terminal(task['id'])
            ended, _ = live.wait_pane('H3X-EXIT', tid, lambda lines: any(END_LINE in line for line in lines), TIMEOUTS['endLine'], 'opencode')
            row = {'text': key, 'send': sent['result'], 'code': sent.get('code'), 'exited': ended is not None}
            if ended is not None:
                before = live.get(task['id']).get('sessionId')
                resumed = live.rpc('agents.resume', {'id': task['id']})
                row['sameSession'] = resumed.get('sessionId') == before
                live.watch('H3X-EXIT', task['id'], ('idle',), TIMEOUTS['resume'], 'opencode')
            rows.append(row)
        live.put('H3X-EXIT', 'texts', rows)
        conclude(live, 'H3X-EXIT', {'recorded': len(rows) == 3}, [], record_only=True)

    def env(live, tasks):
        task = live.create('H3X-ENV', 'opencode', 'P-TOOLS', 'P-ENV')
        tasks.append(task['id'])
        live.watch('H3X-ENV', task['id'], ('idle',), TIMEOUTS['turn'], 'opencode')
        tid = live.terminal(task['id'])
        lines = live.capture('H3X-ENV', tid, 'opencode')
        names = lambda ls: {n for n in ('DEVBOX_BIN', 'DEVBOX_INSTANCE', 'DEVBOX_AGENT_ID', 'DEVBOX_AGENT_LAUNCH') if any(n + '=' in line or n + ' ' in line or line.strip() == n for line in ls)}
        tool_names = names(lines)
        # D2: Enter only after the input row is exactly the command; without that (b) was not run.
        sent = live.command('H3X-ENV', tid, "!env | grep '^DEVBOX_'", 'opencode')
        shown = None
        if sent:
            shown, _ = live.wait_pane('H3X-ENV', tid, lambda ls: 'DEVBOX_INSTANCE' in '\n'.join(ls[-30:]), TIMEOUTS['screen'], 'opencode')
        shell_names = names(shown or [])
        scope, scope_error = scope_env_names(task['id'])
        live.put('H3X-ENV', 'names', {'tool': sorted(tool_names), 'shell': sorted(shell_names), 'scope': scope, 'scopeError': scope_error})
        parts = {'a': {'DEVBOX_BIN', 'DEVBOX_INSTANCE'} <= tool_names and not tool_names & {'DEVBOX_AGENT_ID', 'DEVBOX_AGENT_LAUNCH'},
                 'b': (shown is not None and shell_names == tool_names) if sent else None, 'c': scope is not None}
        reasons = [row_text('H3X-ENV.editor')]
        if not sent:
            reasons.append('The shell command was not shown as the input row; no Enter was sent and (b) was not checked')
        conclude(live, 'H3X-ENV', parts, reasons)

    def nest(live, tasks):
        live.budget.nest('opencode')
        task = live.create('H3X-NEST', 'opencode', 'P-TOOLS', 'P-NEST')
        tasks.append(task['id'])
        before = live.get(task['id']).get('sessionId')
        rows, after = live.watch('H3X-NEST', task['id'], ('idle', 'failed'), TIMEOUTS['nested'], 'opencode')
        live.put('H3X-NEST', 'session', {'before': before, 'after': after.get('sessionId'), 'state': after.get('state'), 'toolExited': after.get('toolExited')})
        conclude(live, 'H3X-NEST', {'a': after.get('sessionId') == before and after.get('state') == 'idle' and not after.get('toolExited')}, [])

    steps = [('H3', h3), ('H3X-REPORT', report), ('H3X-CONFIG', config), ('H3X-SHELL', shell), ('SN1', sn1), ('SN2', sn2),
             ('H3X-EXIT', exits), ('H3X-ENV', env), ('H3X-NEST', nest)]
    run_block(live, 'opencode', selected, steps)


def scope_env_names(agent_id):
    """Each process of the task's scope: whether the key DEVBOX_AGENT_ID is present (key names only).

    Returns (rows, None), or (None, the bounded, redacted reason) when the scope cannot be read."""
    unit = 'devbox-accept-agent-' + agent_id + '.scope'
    result = subprocess.run(['systemctl', '--user', 'show', '-p', 'ControlGroup', '--value', unit], capture_output=True, text=True, timeout=10,
                            env={'XDG_RUNTIME_DIR': f'/run/user/{os.getuid()}', 'DBUS_SESSION_BUS_ADDRESS': f'unix:path=/run/user/{os.getuid()}/bus', 'PATH': '/usr/bin:/bin'})
    group = result.stdout.strip()
    if result.returncode or not group.startswith('/'):
        return None, 'systemctl show exit %d: %s' % (result.returncode, redact_text(result.stderr, 400))
    rows = []
    for pid in (Path('/sys/fs/cgroup' + group) / 'cgroup.procs').read_text().split():
        try:
            keys = {entry.split(b'=', 1)[0] for entry in (Path('/proc') / pid / 'environ').read_bytes().split(b'\0') if entry}
            comm = (Path('/proc') / pid / 'comm').read_text().strip()
        except OSError:
            continue
        rows.append({'pid': int(pid), 'comm': comm[:32], 'hasAgentId': b'DEVBOX_AGENT_ID' in keys})
    return rows, None


def sh_unquote(value):
    parts = shlex.split(value)
    if len(parts) != 1:
        raise ValueError('Expected one shell word')
    return parts[0]


def opencode_config(live, task):
    """H3 추가: merged plugin list = baseline list + exactly the start script's plugin. Counts, booleans, hashes only."""
    data = live.home / '.local/share/devbox/accept/agents' / task['id'] / 'start.sh'
    script = data.read_text()
    content = path_dir = None
    for line in script.splitlines():
        if line.startswith('export OPENCODE_CONFIG_CONTENT='):
            content = sh_unquote(line[len('export OPENCODE_CONFIG_CONTENT='):])
        # start.rs:63-72: `export PATH=<dir>:"$PATH"`, or `export PATH="$PATH":<dir>` for a shared folder.
        if line.startswith('export PATH="$PATH":'):
            path_dir = sh_unquote(line[len('export PATH="$PATH":'):])
        elif line.startswith('export PATH=') and line.endswith(':"$PATH"'):
            path_dir = sh_unquote(line[len('export PATH='):-len(':"$PATH"')])
    if content is None or path_dir is None:
        return {'a': None}, ['start.sh has no OPENCODE_CONFIG_CONTENT or PATH line']
    ours = json.loads(content).get('plugin') or []
    worktree = live.get(task['id']).get('worktreePath')
    env = {'HOME': str(live.home), 'PATH': path_dir + ':/usr/local/bin:/usr/bin:/bin', 'LANG': 'C.UTF-8', 'USER': os.environ.get('USER', '')}

    def plugins(extra):
        result = run_checked([str(Path(path_dir) / 'opencode'), 'debug', 'config'], timeout=60, cwd=worktree, env={**env, **extra})
        value = json.loads(result.stdout[:4 * 1024 * 1024])  # parsed in memory only; never stored or sent
        return [str(p) for p in value.get('plugin') or []]
    base = plugins({})
    merged = plugins({'OPENCODE_CONFIG_CONTENT': content})
    h = lambda items: sorted(hashlib.sha256(i.encode()).hexdigest()[:16] for i in items)
    live.put('H3X-CONFIG', 'plugins', {'baseCount': len(base), 'mergedCount': len(merged), 'oursCount': len(ours), 'base': h(base), 'merged': h(merged),
                                       'ours': h(ours), 'oursIsFileUrl': all(o.startswith('file://') and '/opencode-plugin-' in o and o.endswith('.ts') for o in ours)})
    if not base:
        return {'a': None}, [row_text('H3X-CONFIG.noplugins')]
    extra = [p for p in merged if p not in base]
    return {'a': len(ours) == 1 and sorted(merged) == sorted(base + ours) and extra == ours}, []


# --- pi block: H4, K1 추가(보내기 경로), SN2 pi part. ---------------------------------------------------
def pi_block(live, selected, client):
    state = {}

    def ensure(live, tasks, case):
        if 'task' not in state:
            task = live.create(case, 'pi', 'P-TOOLS', 'P-OK')
            tasks.append(task['id'])
            state['task'] = task
            state['launch'], _ = live.watch(case, task['id'], ('idle',), TIMEOUTS['turn'], 'pi')
        return state['task']

    def send_path(live, tasks):
        task = ensure(live, tasks, 'K1X-SEND')
        tid = live.terminal(task['id'])
        version = live.pane.display('#{version}', tid)
        keys = live.pane.show_extended_keys()
        live.put('K1X-SEND', 'tmux', {'version': version, 'extendedKeys': keys})
        sent = live.send('K1X-SEND', 'pi', task['id'], 'P-OK')
        rows, _ = live.watch('K1X-SEND', task['id'], ('idle',), TIMEOUTS['turn'], 'pi')
        lines = live.capture('K1X-SEND', tid, 'pi')
        text = text_for(live.nonce, 'P-OK')
        submitted = any(text in line for line in lines[:-3]) and not any(text in line for line in lines[-3:])
        numbers = [int(x) for x in re.findall(r'\d+', version)[:2]] or [0]
        parts = {'a': tuple(numbers + [0])[:2] >= (3, 5) and 'on' in keys, 'b': sent['result'] == 'accepted' and submitted and went(rows, 'running', 'idle')}
        conclude(live, 'K1X-SEND', parts, [])

    def sn2(live, tasks):
        task = ensure(live, tasks, 'SN2')
        sn2_part(live, 'pi', task['id'], client)

    def h4(live, tasks):
        task = ensure(live, tasks, 'H4')
        parts = h_checks(live, 'H4', 'pi', task, state['launch'])
        reasons = [live_reason('R-TOAST')] + ([row_text('H4.noquit')] if parts.get('b') is None else [])
        conclude(live, 'H4', parts, reasons)

    # M8: one cap for all paths (pi 5): create 1 + K1X-SEND 1, then H4 (no turn), then SN2's four sends last, so only
    # SN2's pi part meets the cap: its last MCP send is kept as `not sent` and SN2's pi part stays partial.
    run_block(live, 'pi', selected, [('K1X-SEND', send_path), ('H4', h4), ('SN2', sn2)])


# --- claude: H1 추가(신뢰 대화), then with Q1 H1, 셸 모드, ? 도움말, P1, SN2 claude part. ------------------
def trust_screen(tool, lines):
    text = '\n'.join(lines)
    if tool == 'claude':
        return all(s in text for s in CLAUDE_TRUST_TEXT)
    return 'Folder access' in text and bool(CODEX_TRUST_SCREEN.search(text))


def screen_text(lines):
    """The screen without codex composer lines (D8): a typed `/feedback` or `/goal ...` must not satisfy its own screen predicate."""
    return '\n'.join(line for line in lines if not line.lstrip().startswith('›'))


def accept_trust(live, case, tool, tid, project):
    """Rule (ii): with the answer yes, move to the trust option, re-capture, then Enter once per project."""
    if tool == 'claude':
        lines = live.capture(case, tid, tool)
        if not CLAUDE_TRUST_SELECTED.search('\n'.join(lines)):
            live.press(case, tid, 'Down')
            deadline = live.clock.monotonic() + TIMEOUTS['trustSelect']
            while True:
                lines = live.capture(case, tid, tool)
                if CLAUDE_TRUST_SELECTED.search('\n'.join(lines)):
                    break
                if live.clock.monotonic() >= deadline:
                    raise Refused('claude trust option not selected within 3 s after Down')
                live.clock.sleep(POLL)
        live.press(case, tid, 'Enter', 'trust-claude', project)
    else:
        live.capture(case, tid, tool)
        live.press(case, tid, 'Enter', 'trust-codex', project)


UNREAD = object()  # trust_row's config_before when the caller did not read the codex hash


def end_code(lines, task=None):
    """The code of the product's end line (crates/agents/src/start.rs:132), else the task's `exit:N` reason
    (state.rs:86), else None."""
    for line in reversed(lines or []):
        match = END_CODE.search(line) if END_LINE in line else None
        if match:
            return int(match.group(1))
    reason = EXIT_REASON.fullmatch(str((task or {}).get('reason') or ''))
    return int(reason.group(1)) if reason else None


def trust_row(live, case, tool, task, client, state, config_before=UNREAD):
    """H1 추가(신뢰 대화) / H2 추가: observe the first task's folder-trust screen; no key on it.

    Round 2 item 3 (Ruling 85): (b)'s attention bound is TIMEOUTS['trustAttention'], from the first capture that showed
    the trust screen. Item 4: the product's end line on the pane, or an ended task (failed, review or closed, or
    toolExited), before any trust screen ends the row NOT_RUN with the toolExit sentence; its code and the redacted
    last STOP_LINES lines are kept as `toolExit`. N2: the codex hash is read by the caller before agents.create
    (`config_before`); the claude flag and the codex hash are recorded on every path."""
    if tool == 'codex' and config_before is UNREAD:
        config_before = live.codex_config_sha256()
    row = 'H1X-TRUST' if tool == 'claude' else 'H2X-TRUST'
    tid = live.terminal(task['id'])
    seen = {}

    def shown(lines):
        if trust_screen(tool, lines):
            seen['screen'] = live.clock.monotonic()  # the first capture that showed it; no agents.get in between
            return True
        if any(END_LINE in line for line in lines):
            seen['ended'] = None
            return True
        current = live.get(task['id'])
        if current.get('state') in ENDED_STATES or current.get('toolExited'):
            seen['ended'] = current
            return True
        return False

    def trust_files():
        """The claude flag (hasTrustDialogAccepted for the project or the task folder) or the codex hash; True when
        neither changed what this row tests."""
        if tool == 'claude':
            project = live.load()['projectPaths']['P-TOOLS']
            worktree = live.get(task['id']).get('worktreePath')
            trusted = {'project': live.claude_trusted(project), 'worktree': live.claude_trusted(worktree)}
            live.put(case, 'trustEntryPresent', {k: v is not None for k, v in trusted.items()})
            live.put(case, 'trustFlagSet', any(trusted.values()))
            return not any(trusted.values())
        after = live.codex_config_sha256()
        live.put(case, 'configSha256', {'before': config_before, 'after': after, 'unchanged': after == config_before})
        return after == config_before

    lines, seconds = live.wait_pane(case, tid, shown, TIMEOUTS['screen'], tool)
    live.put(case, 'trustScreen', {'seen': 'screen' in seen, 'seconds': seconds if 'screen' in seen else None})
    if 'ended' in seen:
        ended = seen['ended']
        code = end_code(lines, ended)
        if code is None and ended is None:
            ended = live.get(task['id'])
            code = end_code([], ended)
        kept = list(lines)
        while kept and not kept[-1].strip():
            kept.pop()  # the pane below the last output is blank; the last lines are the tool's
        exit_row = {'code': code, 'lastLines': bounded(kept[-STOP_LINES:])}
        if ended is not None:
            exit_row['task'] = {k: ended.get(k) for k in ('state', 'reason', 'toolExited')}
        live.put(case, 'toolExit', exit_row)
        trust_files()
        live.finish(case, 'NOT_RUN', row_text(row + '.toolExit').format(code='?' if code is None else code))
        return False
    if lines is None:
        trust_files()
        live.finish(case, 'NOT_RUN', row_text(row + '.absent'))
        return False
    checks = waiting_checks(live, case, tool, task['id'], tid, client, bound=TIMEOUTS['trustAttention'], since=seen['screen'])
    parts = {'b': all(checks.values())}
    unchanged = trust_files()
    if tool == 'claude':
        parts['b'] = parts['b'] and unchanged
        fixture_lines = ['Accessing workspace:', 'Yes, I trust this folder', 'No, exit', 'Enter to confirm']
        live.put(case, 'layout', {line: any(line in s for s in lines) for line in fixture_lines})
    else:
        which = [name for name in ('Trust and continue', 'Open restricted', 'Open existing task') if name in '\n'.join(lines)]
        live.put(case, 'screen', which)
        parts['c'] = unchanged
    parts['a'] = True
    state['trustScreen'] = True
    conclude(live, case, parts, [])
    return True


def claude_block(live, selected, client):
    state = {}

    def trust(live, tasks):
        task = live.create('H1X-TRUST', 'claude', 'P-TOOLS', 'P-OK')
        tasks.append(task['id'])
        state['task'] = task
        trust_row(live, 'H1X-TRUST', 'claude', task, client, state)

    def main(live, case, tasks):
        if 'task' not in state:  # H1 추가(신뢰 대화) was not selected: the first claude task starts here
            state['task'] = live.create(case, 'claude', 'P-TOOLS', 'P-OK')
            tasks.append(state['task']['id'])
        if 'launch' not in state:
            if live.answers.get('Q1') is not True:
                raise Refused(live_reason('R-Q1'))
            tid = live.terminal(state['task']['id'])
            lines = live.capture(case, tid, 'claude')
            if trust_screen('claude', lines):
                accept_trust(live, case, 'claude', tid, 'P-TOOLS')
            state['launch'], _ = live.watch(case, state['task']['id'], ('idle',), TIMEOUTS['turn'], 'claude')
        return state['task']

    def h1(live, tasks):
        task = main(live, 'H1', tasks)
        conclude(live, 'H1', h_checks(live, 'H1', 'claude', task, state['launch']), [live_reason('R-TOAST')])

    def shell(live, tasks):
        task = main(live, 'H1X-SHELL', tasks)
        tid = live.terminal(task['id'])
        live.type('H1X-SHELL', tid, '! echo ok')
        lines = live.capture('H1X-SHELL', tid, 'claude')
        footer = any('! for shell mode' in line for line in lines[-6:])
        checks = waiting_checks(live, 'H1X-SHELL', 'claude', task['id'], tid, client)
        draft = live.capture('H1X-SHELL', tid, 'claude')
        live.put('H1X-SHELL', 'layout', {'shellModeFooterInLast6': footer, 'draftKept': any('echo ok' in line for line in draft[-6:])})
        live.press('H1X-SHELL', tid, 'C-u')
        cleared = live.capture('H1X-SHELL', tid, 'claude')
        parts = {'a': footer, 'b': checks['send'] and checks['mcp'] and checks['input0'], 'c': True,
                 'd': not any('echo ok' in line for line in cleared[-6:])}
        conclude(live, 'H1X-SHELL', parts, [])

    def help_row(live, tasks):
        task = main(live, 'H1X-HELP', tasks)
        tid = live.terminal(task['id'])
        live.type('H1X-HELP', tid, '?')
        lines = live.capture('H1X-HELP', tid, 'claude')
        sent = live.send('H1X-HELP', 'claude', task['id'], 'P-OK')
        mcp = live.mcp_send('H1X-HELP', 'claude', client, task['id'], 'P-OK')
        live.press('H1X-HELP', tid, 'Escape')
        live.watch('H1X-HELP', task['id'], ('idle',), TIMEOUTS['turn'], 'claude')
        live.put('H1X-HELP', 'record', {'footerInLast6': any('! for shell mode' in line for line in lines[-6:]), 'send': sent['result'],
                                        'sendCode': sent.get('code'), 'mcp': mcp['result']})
        conclude(live, 'H1X-HELP', {'recorded': True}, [], record_only=True)

    def p1(live, tasks):
        task = main(live, 'P1', tasks)
        conclude(live, 'P1', *approval_row(live, 'P1', task, client), )

    def sn2(live, tasks):
        task = main(live, 'SN2', tasks)
        sn2_part(live, 'claude', task['id'], client)

    def gated(step):
        def run(live, tasks):
            if live.answers.get('Q1') is not True:
                raise Refused(live_reason('R-Q1'))
            step(live, tasks)
        return run

    steps = [('H1X-TRUST', trust), ('H1', gated(h1)), ('H1X-SHELL', gated(shell)), ('H1X-HELP', gated(help_row)), ('P1', gated(p1)),
             ('SN2', gated(sn2))]
    run_q_block(live, 'claude', selected, steps, 'Q1', 'H1X-TRUST')


def run_q_block(live, tool, selected, steps, question, trust_case):
    """The trust row always runs; the answer-gated rows end NOT_RUN with R-Q1/R-Q2 when the answer is no."""
    if live.answers.get(question) is not True:
        gated = [case for case, _ in steps if case != trust_case and case in selected and case != 'SN2']
        for case in gated:
            live.finish(case, 'NOT_RUN', live_reason('R-' + question))
        if 'SN2' in selected:
            live.sn2[tool] = None
        run_block(live, tool, {trust_case} & set(selected), steps[:1])
        return
    run_block(live, tool, selected, steps)


def approval_row(live, case, task, client):
    """P1 / U1 추가 (a)-(e): the claude permission prompt for the exact P-APPROVE command (changes §8.1.2 iii)."""
    tid = live.terminal(task['id'])
    sent = live.send(case, 'claude', task['id'], 'P-APPROVE')
    lines, seconds = live.wait_pane(case, tid, lambda ls: approval_shown(ls) or live.get(task['id']).get('state') == 'idle', TIMEOUTS['turn'], 'claude')
    if lines is None or not approval_shown(lines):
        return {'prompt': None}, [live_reason('R-NOAPPROVAL')]
    checks = waiting_checks(live, case, 'claude', task['id'], tid, client)
    parts = {'a': checks['attention'], 'b': checks['send'] and checks['mcp'], 'c': checks['input0']}
    reasons = []
    if case == 'P1':
        reasons.append(live_reason('R-UI-PREFIX') + row_text('P1.ui'))
    if live.answers.get('Q5') is not True:
        live.press(case, tid, 'Escape')
        parts.update(d=None, e=None)
        reasons.append(live_reason('R-Q5'))
        return parts, reasons
    if case == 'P1':
        # (d) the product-style send (tmux.rs:419-422) on the dialog, once; rule (iii) still has to hold.
        live.pane.paste(tid, 's7-paste-probe')
        live.note(case, 'keys', {'t': round(live.clock.monotonic(), 3), 'paste': 's7-paste-probe'})
        pasted = live.capture(case, tid, 'claude')
        try:
            live.press(case, tid, 'Enter', 'approve')
        except Refused:
            live.put(case, 'pasteEnter', {'enterSent': False, 'dialogAfterPaste': digest_lines(pasted)})
            live.press(case, tid, 'Escape')
            parts.update(d=None, e=None)
            reasons.append('After the paste the dialog no longer showed the plain once approval for the command; no Enter was sent')
            return parts, reasons
        after = live.capture(case, tid, 'claude')
        still = approval_shown(after)
        live.put(case, 'pasteEnter', {'enterSent': True, 'dialogStillOpen': still})
        if still:
            live.press(case, tid, 'Enter', 'approve')
    else:
        live.capture(case, tid, 'claude')
        live.press(case, tid, 'Enter', 'approve')
    rows, _ = live.watch(case, task['id'], ('idle',), TIMEOUTS['turn'], 'claude')
    worktree = Path(live.get(task['id']).get('worktreePath') or '/nonexistent')
    parts['d'] = True
    parts['e'] = went(rows, 'running', 'idle') and (worktree / 's7-approved').is_file()
    live.put(case, 'approvedFile', (worktree / 's7-approved').is_file())
    return parts, reasons


# --- codex: H2 추가, then with Q2 H2 and the remaining H2 추가 rows, P1 추가, SN2 codex part. -------------
CODEX_SCREENS = (('rename', ['/rename'], re.compile(r'(?i)\brename\b')),
                 ('statusline', ['/statusline'], re.compile(r'Configure Status Line')),
                 ('experimental', ['/experimental'], re.compile(r'Experimental features')),
                 ('keymap', ['/keymap'], re.compile(r'(?m)^\s*Keymap\s*$')),
                 ('question', 'P-ASK', re.compile(r'(?m)^\s*\d+ of \d+\s*$')),
                 ('resume-picker', ['/resume'], re.compile(r'Resume a previous session')),
                 ('inline-history', ['C-t'], re.compile(r'T R A N S C R I P T')))


def codex_block(live, selected, client):
    state = {}

    def trust(live, tasks):
        config_before = live.codex_config_sha256()  # N2: before agents.create, so the hash covers the whole task
        task = live.create('H2X-TRUST', 'codex', 'P-TOOLS', 'P-OK')
        tasks.append(task['id'])
        state['task'] = task
        trust_row(live, 'H2X-TRUST', 'codex', task, client, state, config_before=config_before)

    def main(live, case, tasks):
        if 'task' not in state:
            state['task'] = live.create(case, 'codex', 'P-TOOLS', 'P-OK')
            tasks.append(state['task']['id'])
        if 'launch' not in state:
            tid = live.terminal(state['task']['id'])
            lines = live.capture(case, tid, 'codex')
            if trust_screen('codex', lines):
                accept_trust(live, case, 'codex', tid, 'P-TOOLS')
            state['launch'], _ = live.watch(case, state['task']['id'], ('idle',), TIMEOUTS['turn'], 'codex')
        return state['task']

    def h2(live, tasks):
        task = main(live, 'H2', tasks)
        conclude(live, 'H2', h_checks(live, 'H2', 'codex', task, state['launch']), [live_reason('R-TOAST')])

    def exit_tool(live, case, task):
        tid = live.terminal(task['id'])
        if not live.command(case, tid, '/quit', 'codex'):
            return False
        ended, _ = live.wait_pane(case, tid, lambda ls: any(END_LINE in line for line in ls), TIMEOUTS['endLine'], 'codex')
        return ended is not None

    def resume_goal(live, tasks):
        task = main(live, 'H2X-RESUME', tasks)
        tid = live.terminal(task['id'])
        sent = live.command('H2X-RESUME', tid, '/goal Reply with OK', 'codex')
        lines = None
        if sent:
            lines, _ = live.wait_pane('H2X-RESUME', tid, lambda ls: bool(re.search(r'(?i)goal', screen_text(ls[-12:]))), TIMEOUTS['screen'], 'codex')
        if lines is None or re.search(r'(?i)unknown command|unrecognized command', '\n'.join(lines)):
            live.finish('H2X-RESUME', 'NOT_RUN', row_text('H2X-RESUME.nogoal'))
            return
        live.press('H2X-RESUME', tid, 'Escape')
        if not exit_tool(live, 'H2X-RESUME', task):
            live.finish('H2X-RESUME', 'NOT_RUN', 'codex did not end on /quit within 10 s; the resume was not checked')
            return
        resumed = live.rpc('agents.resume', {'id': task['id']})
        tid = resumed.get('terminalId') or live.terminal(task['id'])
        live.pane.resize(tid, 200, 50)
        box, _ = live.wait_pane('H2X-RESUME', tid, lambda ls: 'Resume paused goal?' in '\n'.join(ls), TIMEOUTS['screen'], 'codex')
        parts = {'a': True}
        live.put('H2X-RESUME', 'box', box is not None)
        if box is not None:
            text = '\n'.join(box)
            live.put('H2X-RESUME', 'choices', bool(re.search('Resume goal', text)) and bool(re.search('Leave paused', text)))
            checks = waiting_checks(live, 'H2X-RESUME', 'codex', task['id'], tid, client, bound=TIMEOUTS['trustAttention'])
            parts['b'] = all(checks.values())
        live.pane.resize(tid, 80, 24)
        live.press('H2X-RESUME', tid, 'Escape')
        conclude(live, 'H2X-RESUME', parts, [] if box is not None else ["The 'Resume paused goal?' box did not appear; (b) was not checked"])

    def resume_send(live, tasks):
        task = main(live, 'H2X-RESUME1S', tasks)
        if not exit_tool(live, 'H2X-RESUME1S', task):
            live.finish('H2X-RESUME1S', 'NOT_RUN', 'codex did not end on /quit within 10 s')
            return
        live.rpc('agents.resume', {'id': task['id']})
        returned = live.clock.monotonic()
        sent = live.send('H2X-RESUME1S', 'codex', task['id'], 'P-OK')
        gap = round((live.clock.monotonic() - returned) * 1000 - sent['ms'])
        tid = live.terminal(task['id'])
        text = text_for(live.nonce, 'P-OK')
        where = {}
        for mark in (5, 30):
            live.clock.sleep(max(0.0, returned + mark - live.clock.monotonic()))
            lines = live.capture('H2X-RESUME1S', tid, 'codex')
            where[str(mark)] = ('input' if any(text in line for line in lines[-4:]) else 'submitted' if any(text in line for line in lines) else 'lost')
        rows, _ = live.watch('H2X-RESUME1S', task['id'], ('idle',), TIMEOUTS['turn'], 'codex')
        live.put('H2X-RESUME1S', 'record', {'msResumeToSend': gap, 'send': sent['result'], 'code': sent.get('code'), 'where': where})
        conclude(live, 'H2X-RESUME1S', {'recorded': gap <= 1000}, [] if gap <= 1000 else ['The send left later than 1 s after the resume'],
                 record_only=True)

    def feedback(live, tasks):
        task = main(live, 'H2X-FEEDBACK', tasks)
        tid = live.terminal(task['id'])
        sent = live.command('H2X-FEEDBACK', tid, '/feedback', 'codex')
        lines = None
        if sent:
            lines, _ = live.wait_pane('H2X-FEEDBACK', tid, lambda ls: bool(re.search(r'(?i)feedback|Tell us more', screen_text(ls))), TIMEOUTS['screen'], 'codex')
        if lines is None:
            live.finish('H2X-FEEDBACK', 'NOT_RUN', row_text('H2X-FEEDBACK.noscreen'))
            return
        checks = waiting_checks(live, 'H2X-FEEDBACK', 'codex', task['id'], tid, client)
        live.press('H2X-FEEDBACK', tid, 'Escape')
        back, _ = live.wait_pane('H2X-FEEDBACK', tid, lambda ls: not re.search(r'Tell us more', screen_text(ls)), TIMEOUTS['screen'], 'codex')
        conclude(live, 'H2X-FEEDBACK', {'a': True, 'b': checks['send'] and checks['mcp'] and checks['input0'], 'c': back is not None}, [])

    def screens(live, tasks):
        task = main(live, 'H2X-SCREENS', tasks)
        start_hash = live.codex_config_sha256()
        rows, missing = [], []
        for name, opener, pattern in CODEX_SCREENS:
            if name == 'experimental' and any(r['screen'] == 'statusline' and r['opened'] for r in rows):
                continue
            tid = live.terminal(task['id'])
            sent = True
            if opener == 'P-ASK':
                live.send('H2X-SCREENS', 'codex', task['id'], 'P-ASK')
            elif opener[0].startswith('/'):
                sent = live.command('H2X-SCREENS', tid, opener[0], 'codex')
            else:
                live.press('H2X-SCREENS', tid, opener[0])
            lines = None
            if sent:
                lines, _ = live.wait_pane('H2X-SCREENS', tid, lambda ls: bool(pattern.search(screen_text(ls))), TIMEOUTS['turn'] if opener == 'P-ASK' else TIMEOUTS['screen'], 'codex')
            row = {'screen': name, 'opened': lines is not None}
            if lines is not None:
                checks = waiting_checks(live, 'H2X-SCREENS', 'codex', task['id'], tid, client, bound=TIMEOUTS['trustAttention'])
                row.update(attention=checks['attention'], refused=checks['send'] and checks['mcp'])
            else:
                missing.append(row_text('H2X-SCREENS.noscreen').replace('<화면>', name))
            live.press('H2X-SCREENS', tid, 'Escape')
            rows.append(row)
        tid = live.terminal(task['id'])
        live.press('H2X-SCREENS', tid, 'Escape')
        live.press('H2X-SCREENS', tid, 'Escape')
        history = live.send('H2X-SCREENS', 'codex', task['id'], 'P-OK')
        live.watch('H2X-SCREENS', task['id'], ('idle',), TIMEOUTS['turn'], 'codex')
        rows.append({'screen': 'fullscreen-history', 'sendAccepted': history['result'] == 'accepted'})
        same = live.codex_config_sha256() == start_hash
        live.put('H2X-SCREENS', 'screens', rows)
        live.put('H2X-SCREENS', 'configUnchanged', same)
        opened = [r for r in rows if r.get('opened')]
        parts = {'screens': all(r.get('attention') and r.get('refused') for r in opened) if opened else None,
                 'history': history['result'] == 'accepted', 'config': same}
        conclude(live, 'H2X-SCREENS', parts, missing)

    def shellmark(live, tasks):
        task = main(live, 'H2X-SHELLMARK', tasks)
        rows = []
        for variant in ('narrow', 'side', 'help'):
            tid = live.terminal(task['id'])
            if variant == 'narrow':
                live.type('H2X-SHELLMARK', tid, '!echo s7')
                live.pane.resize(tid, 40, 24)
            elif variant == 'side':
                # D2: `/side` goes through Live.command (Enter only on the exact input row); the next text waits for the pane.
                opened = live.command('H2X-SHELLMARK', tid, '/side', 'codex')
                live.type('H2X-SHELLMARK', tid, '!echo s7')
            else:
                live.type('H2X-SHELLMARK', tid, '?')
                live.type('H2X-SHELLMARK', tid, '!echo s7')
            lines = live.capture('H2X-SHELLMARK', tid, 'codex')
            sent = live.send('H2X-SHELLMARK', 'codex', task['id'], 'P-OK')
            mcp = live.mcp_send('H2X-SHELLMARK', 'codex', client, task['id'], 'P-OK')
            rows.append({'variant': variant, 'shellMode': any(re.search(r'[ \t]{2}Shell mode[ \t]*$', line) for line in lines), 'send': sent['result'],
                         'code': sent.get('code'), 'mcp': mcp['result'], **({'sideSubmitted': opened} if variant == 'side' else {})})
            live.press('H2X-SHELLMARK', tid, 'C-u')
            live.press('H2X-SHELLMARK', tid, 'Escape')
            live.pane.resize(tid, 80, 24) if variant == 'narrow' else None
            live.watch('H2X-SHELLMARK', task['id'], ('idle',), TIMEOUTS['turn'], 'codex', settle=0.5)
        live.put('H2X-SHELLMARK', 'variants', rows)
        conclude(live, 'H2X-SHELLMARK', {'recorded': len(rows) == 3}, [], record_only=True)

    def p1x(live, tasks):
        task = main(live, 'P1X', tasks)
        tid = live.terminal(task['id'])
        live.send('P1X', 'codex', task['id'], 'P-APPROVE-X')
        overlay = re.compile(r'(?i)(?:Would you like to run|Allow command|approve|Yes, proceed)')
        lines, _ = live.wait_pane('P1X', tid, lambda ls: bool(overlay.search('\n'.join(ls))) or live.get(task['id']).get('state') == 'idle', TIMEOUTS['turn'], 'codex')
        if lines is None or not overlay.search('\n'.join(lines)):
            live.finish('P1X', 'NOT_RUN', row_text('P1X.noapproval'))
            return
        checks = waiting_checks(live, 'P1X', 'codex', task['id'], tid, client, bound=TIMEOUTS['trustAttention'])
        live.press('P1X', tid, 'Escape')
        conclude(live, 'P1X', {'a': checks['attention'], 'b': checks['send'] and checks['mcp'], 'c': checks['input0']},
                 [live_reason('R-UI-PREFIX') + row_text('P1.ui')])

    def sn2(live, tasks):
        task = main(live, 'SN2', tasks)
        sn2_part(live, 'codex', task['id'], client)

    steps = [('H2X-TRUST', trust), ('H2', h2), ('H2X-RESUME', resume_goal), ('H2X-RESUME1S', resume_send), ('H2X-FEEDBACK', feedback),
             ('H2X-SCREENS', screens), ('H2X-SHELLMARK', shellmark), ('P1X', p1x), ('SN2', sn2)]
    run_q_block(live, 'codex', selected, steps, 'Q2', 'H2X-TRUST')


def record_sn2(live):
    """SN2 once, after the tool blocks: PASS only when all four tool parts were recorded in full (no `not sent` row)."""
    # D9 (Ruling 153): the "not allowed" reason only when the answer is not true; otherwise the block ended before SN2.
    reasons = {tool: live_reason(code) if live.answers.get(q) is not True else tool + ' part not recorded: the ' + tool + ' block ended before SN2'
               for tool, code, q in (('claude', 'R-Q1', 'Q1'), ('codex', 'R-Q2', 'Q2'))}
    live.put('SN2', 'parts', live.sn2)
    full = lambda rows: bool(rows) and all(row.get('result') != 'not sent' for row in rows)
    done = [tool for tool in TOOLS if full(live.sn2.get(tool))]
    if len(done) == len(TOOLS):
        live.finish('SN2', 'PASS', 'record-only observations recorded')
    else:
        missing = [(tool + ' part partial: a send was over the turn budget (M8)') if live.sn2.get(tool) else reasons.get(tool, tool + ' part not run')
                   for tool in TOOLS if tool not in done]
        live.finish('SN2', 'NOT_RUN', ' '.join(missing), partial=done)


# --- U1, U1 추가, CS5 tool parts (upgrade flows). ----------------------------------------------------
def u1_prepare(live, state, client_factory):
    """rc.1 phase: claude (Q1) and codex (Q2) tasks in P-U1, and the rc.1 MCP process kept open."""
    require_lock()
    live.pane.begin('U1')
    for tool, question in (('claude', 'Q1'), ('codex', 'Q2')):
        if live.answers.get(question) is not True:
            state[tool] = None
            continue
        task = live.create('U1', tool, 'P-U1', 'P-OK')
        state.setdefault('tasks', []).append(task['id'])
        tid = live.terminal(task['id'])
        lines, _ = live.wait_pane('U1', tid, lambda ls: trust_screen(tool, ls) or live.get(task['id']).get('state') in ('running', 'idle'), TIMEOUTS['start'], tool)
        if lines is not None and trust_screen(tool, lines):
            accept_trust(live, 'U1', tool, tid, 'P-U1')
        live.watch('U1', task['id'], ('idle',), TIMEOUTS['turn'], tool)
        task = live.get(task['id'])
        state[tool] = {'id': task['id'], 'terminalId': task.get('terminalId'), 'sessionId': task.get('sessionId')}
        if tool == 'claude':
            state['claudeSettings'] = start_settings(live, task['id'])
    state['rc1Mcp'] = client_factory()
    return state


def start_settings(live, agent_id):
    script = (live.home / '.local/share/devbox/accept/agents' / agent_id / 'start.sh').read_text()
    match = re.search(r"--settings '([^']+)'", script)
    return match.group(1) if match else None


def u1_rows(live, state, selected, client):
    """After the rc.2 phase started: U1 (a)-(d), U1 추가 and CS5's hook half (b)."""
    require_lock()
    live.pane.begin('U1')
    parts, reasons = {}, []
    if state.get('claude') is None:
        reasons.append(live_reason('R-Q1'))
    if state.get('codex') is None:
        reasons.append(live_reason('R-Q2'))
    alive = []
    for tool in ('claude', 'codex'):
        old = state.get(tool)
        if not old:
            continue
        task = live.get(old['id'])
        alive.append(live.pane.has_session(old['terminalId']) and task.get('terminalId') == old['terminalId']
                     and task.get('sessionId') == old['sessionId'] and task.get('state') not in ('failed', 'closed'))
    parts['a'] = all(alive) if alive else None
    if state.get('claude'):
        sent = live.send('U1', 'claude', state['claude']['id'], 'P-OK')
        rows, _ = live.watch('U1', state['claude']['id'], ('idle',), TIMEOUTS['u1Turn'], 'claude')
        parts['b'] = sent['result'] == 'accepted' and went(rows, 'running', 'idle')
        task = live.create('U1', 'claude', 'P-U1', 'P-OK')
        state.setdefault('tasks', []).append(task['id'])
        tid = live.terminal(task['id'])
        lines = live.capture('U1', tid, 'claude')
        if trust_screen('claude', lines):
            parts['c'] = None
            reasons.append('claude asked for folder trust again in P-U1; not answered (at most once per project)')
        else:
            rows, after = live.watch('U1', task['id'], ('idle',), TIMEOUTS['turn'], 'claude')
            journal = owned_journal_lines()
            parts['c'] = after.get('state') == 'idle' and 'existing file differs' not in json.dumps(after) and not any('existing file differs' in line for line in journal)
    else:
        parts['b'] = parts['c'] = None
    old_mcp = state.get('rc1Mcp')
    if old_mcp is not None:
        # Round 4 item 4 (review 5, Note A): (d) is record only (s7-14-matrix.md:193). A call that raises or times out is
        # kept, redacted and bounded, as rc1Mcp.error. (d) is then not run, never a U1 FAIL by itself and never a PASS of
        # (d), and U1 concludes from (a)-(c).
        try:
            listing = old_mcp.tools_list()
            agents = old_mcp.call('devbox_agents', {})
        except Exception as error:
            live.put('U1', 'rc1Mcp', {'error': '%s: %s' % (type(error).__name__, redact_text(str(error), 300))})
            parts['d'] = None
            reasons.append('the rc.1 MCP process did not answer tools/list or devbox_agents; (d) is record only and keeps its error as rc1Mcp.error')
        else:
            text = json.dumps([listing, agents], ensure_ascii=False)
            # M7: kept redacted (common.redact_json); the two booleans are read from the reply itself.
            live.put('U1', 'rc1Mcp', {'lines': redact_json([listing, agents]), 'mentionsOpencode': 'opencode' in text, 'mentionsPi': '"pi"' in text})
            parts['d'] = True
        dump(live.cases / 'U1-mcp.json', old_mcp.pairs)
    else:
        parts['d'] = None
    if live.windows_half:
        reasons.append(live.windows_half + ' (' + row_text('U1.windows') + ')')
    reasons = list(dict.fromkeys(reasons))
    if 'U1' in selected:
        conclude(live, 'U1', parts, reasons)
    if 'CS5' in selected:
        state['cs5HookHalf'] = parts.get('b')
    if 'U1X' in selected:
        if state.get('claude') is None:
            live.finish('U1X', 'NOT_RUN', live_reason('R-Q1'))
        elif live.answers.get('Q5') is not True:
            live.finish('U1X', 'NOT_RUN', live_reason('R-Q5'))
        else:
            u1x(live, state, client)


def wait_approval(live, case, task_id, tid):
    """The claude permission prompt for the exact P-APPROVE command with the plain "Yes" selected, or None."""
    lines, _ = live.wait_pane(case, tid, lambda ls: approval_shown(ls) or live.get(task_id).get('state') == 'idle', TIMEOUTS['turn'], 'claude')
    return lines if lines is not None and approval_shown(lines) else None


def u1x(live, state, client):
    """U1 추가 (PROGRESS.md:227): the rc.1-started claude task keeps rc.1 hooks until it is resumed."""
    live.pane.begin('U1X')
    task_id = state['claude']['id']
    tid = live.terminal(task_id)
    reasons = [live.windows_half] if live.windows_half else []
    live.send('U1X', 'claude', task_id, 'P-APPROVE')
    if wait_approval(live, 'U1X', task_id, tid) is None:
        live.finish('U1X', 'NOT_RUN', ' '.join([live_reason('R-NOAPPROVAL')] + reasons))
        return
    live.capture('U1X', tid, 'claude')
    live.press('U1X', tid, 'Enter', 'approve')
    # (a) after the approval key, before the turn ends: still refused (rc.1 hook settings).
    sent = live.send('U1X', 'claude', task_id, 'P-OK')
    mcp = live.mcp_send('U1X', 'claude', client, task_id, 'P-OK')
    parts = {'a': sent['result'] == 'refused' and sent.get('code') == 'agents.awaiting_input'
                  and mcp['result'] == 'refused' and mcp['reply'].startswith('에이전트가 선택을 기다리고 있습니다.')}
    # (b) once Idle, agents.send is accepted.
    rows, after = live.watch('U1X', task_id, ('idle',), TIMEOUTS['turn'], 'claude')
    worktree = Path(live.get(task_id).get('worktreePath') or '/nonexistent')
    live.put('U1X', 'approvedFile', (worktree / 's7-approved').is_file())
    accepted = live.send('U1X', 'claude', task_id, 'P-OK')
    live.watch('U1X', task_id, ('idle',), TIMEOUTS['turn'], 'claude')
    parts['b'] = after.get('state') == 'idle' and accepted['result'] == 'accepted'
    # (c) after /exit and agents.resume, start.sh names the rc.2 hook file; a second approval is not followed by awaiting_input.
    live.command('U1X', tid, '/exit', 'claude')
    ended, _ = live.wait_pane('U1X', tid, lambda ls: any(END_LINE in line for line in ls), TIMEOUTS['endLine'], 'claude')
    if ended is None:
        parts['c'] = None
        reasons.append('claude did not end on /exit within 10 s; (c) was not checked')
    else:
        live.rpc('agents.resume', {'id': task_id})
        live.watch('U1X', task_id, ('idle',), TIMEOUTS['resume'], 'claude')
        settings = start_settings(live, task_id)
        live.put('U1X', 'hookFile', {'before': state.get('claudeSettings'), 'after': settings, 'exists': bool(settings) and Path(settings).is_file()})
        tid = live.terminal(task_id)
        live.send('U1X', 'claude', task_id, 'P-APPROVE')
        if wait_approval(live, 'U1X', task_id, tid) is None:
            parts['c'] = None
            reasons.append(live_reason('R-NOAPPROVAL'))
        else:
            live.capture('U1X', tid, 'claude')
            live.press('U1X', tid, 'Enter', 'approve')
            second = live.send('U1X', 'claude', task_id, 'P-OK')
            live.put('U1X', 'afterSecondApproval', {'result': second['result'], 'code': second.get('code')})
            live.watch('U1X', task_id, ('idle',), TIMEOUTS['turn'], 'claude')
            parts['c'] = (bool(settings) and settings != state.get('claudeSettings') and Path(settings).is_file()
                          and second.get('code') != 'agents.awaiting_input')
    conclude(live, 'U1X', parts, reasons)


def owned_journal_lines():
    """The owned daemon's journal; a failed read raises (an empty journal would pass U1 (c) on no evidence)."""
    result = run_checked(['journalctl', '--user', '-u', 'devbox-accept@accept.service', '-n', '500', '--no-pager', '-o', 'cat'], timeout=20,
                         text=True, env={'XDG_RUNTIME_DIR': f'/run/user/{os.getuid()}', 'PATH': '/usr/bin:/bin'})
    return result.stdout.splitlines()[-500:]
