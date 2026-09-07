# 이전 및 백업

Owner: `control-center` · route: `migration` · status: foundation

Native authority: `shell-read`; fixture: [control-center.migration](../../packages/product-shell/fixtures/features/control-center.migration.json).

This registers a pending feature in the hidden development shell.
Domain implementation, data ownership, migration and acceptance remain pending.

Windows native development: pass `--route=migration` to the debug product executable.
Browser development in `apps/devbox-control-center` accepts `?route=migration` and is
explicitly labelled synthetic. No legacy executable is dispatched.
