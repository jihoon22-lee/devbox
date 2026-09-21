# Knowledge editor recovery (review stage 2)

- Prerequisite: stage 1 PR #570 merged as 5949eb51. CI 35594263069 PASS;
  native 35594263093 PASS after retrying only the WSL2 job (first hosted shell prompt timeout).
- F2: product-owned in-memory NoteDocument and quit registration survive feature/root
  error boundaries. Recovery offers current text and revision-checked save. Feature render
  and rejected lazy imports cannot unmount the other retained features.
- F3/F4: invalidate preview effects and inspect generations across newer work, document/path
  changes and native revisions. Both success and failure replies are guarded.
- F7: split URL path/query/fragment, decode once, reject malformed/escaping paths; preserve
  backend wikilink authority. Navigate to generated/preserved heading IDs after rendering.
- Fixtures: actual Knowledge tree with dirty Notes, Activity render/Search import/editor
  failures; preview reverse completion/A-B-A/mode exit/unmount; inspect ordering;
  encoded links and deferred anchor movement.
- Validation: pending whole-branch affected checks and CI. No Windows GUI PASS claimed yet.
