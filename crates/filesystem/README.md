
`atomic_write_with_outcome` returns only after replacement commits and reports a
post-commit parent-sync failure as `WriteOutcome.durability_warning`. The legacy
`atomic_write` convenience wrapper discards that optional warning but never turns
it into an unsaved-file error. Editor's revision-checked save uses the same
`finish_replacement` contract while retaining its identity/permissions checks.

`collect_limited` bounds visited entries independently of eligible files (at least
1024, otherwise four times the file budget). `collect_bounded` accepts both budgets.
Truncation can therefore mean an unfinished empty-directory scan; consumers must
not use it to infer deletions. Ignored directory entries also consume work budget.

Unix `atomic_write` creates staging files as 0600, preserves an existing target's mode
before publication, and uses 0600 for new files. Windows generic atomic writes retain
platform inheritance; Knowledge's conditional document publication separately preserves
the replaced file DACL and retains displaced data. Post-publication durability warnings
remain distinct from an uncommitted write.
