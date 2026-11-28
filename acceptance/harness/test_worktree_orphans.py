"""Read-only residue detection for native children launched from an acceptance checkout."""
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from teardown import worktree_orphans


class WorktreeOrphans(unittest.TestCase):
    def test_only_new_live_same_user_orphan_in_exact_checkout_is_reported(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            proc = root / 'proc'
            proc.mkdir()
            worktree = root / 'candidate'
            worktree.mkdir()
            other = root / 'candidate-extra'
            other.mkdir()
            for pid, parent, start, cwd, state in [
                    (101, 1, 105, worktree, 'S'), (102, 45, 105, worktree, 'S'),
                    (103, 1, 99, worktree, 'S'), (104, 1, 105, other, 'S'),
                    (105, 1, 106, worktree, 'Z')]:
                entry = proc / str(pid)
                entry.mkdir()
                (entry / 'stat').write_text(f"{pid} (possibly secret ) command) {state} {parent} " + '0 ' * 17 + str(start))
                (entry / 'cwd').symlink_to(cwd)
            identity = worktree.stat()
            worktree.rename(root / 'renamed')
            for p in (proc / str(n) / 'cwd' for n in (101, 102, 103, 105)):
                p.unlink()
                p.symlink_to(root / 'renamed')
            (proc / '106').mkdir()  # exits before /proc/<pid>/stat can be read
            result = worktree_orphans(identity, 100, proc)
            self.assertEqual(result['status'], 'FAIL')
            self.assertEqual(result['retainedResources'], ['WSL PID 101 (start tick 105)'])
            with patch('teardown.os.getuid', return_value=os.getuid() + 1):
                self.assertEqual(worktree_orphans(identity, 100, proc)['status'], 'PASS')
            self.assertTrue((proc / '101' / 'stat').exists())

    def test_incomplete_scan_fails_closed(self):
        with tempfile.TemporaryDirectory() as tmp:
            proc = Path(tmp)
            (proc / '100').mkdir()
            with patch('teardown.ORPHAN_SCAN_LIMIT', 0), self.assertRaises(RuntimeError):
                worktree_orphans(proc.stat(), 0, proc)
