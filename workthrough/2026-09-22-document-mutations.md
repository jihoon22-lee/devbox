# Document mutation ownership (574, stage 1)

- Scope: R1; deletion completion owns only the buffer approved before native dispatch.
- Capture document/edit/open/save generations and path in a one-shot completion ticket.
  Reopened documents and other notes survive; overlapping saves retain dirty content.
- Tests: delayed deletion through the actual context menu; A/B/A and directory boundaries,
  post-approval edits, pending opens, duplicate completion, save/delete completion orders.
- Implementation and fixtures complete. `pnpm verify:affected` PASS: 220 feature tests,
  25 product tests, selected builds/type and bundle checks. PR CI/native acceptance pending.
- Remaining stages: R2/R6/write outcomes; R3/R4; R5/protocol diagnostics; R7/R8.
