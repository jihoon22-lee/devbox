// Runs inside the disposable product renderer. Never retain arguments, payloads,
// paths, arbitrary error text, or identifiers in diagnostic records.
export function installFileIpcDiagnostic(revision) {
  const bridge = window.__TAURI_INTERNALS__;
  const original = bridge.invoke;
  const records = [];
  const states = new Set(["running", "succeeded", "cancelled", "stale", "failed"]);
  const codes = new Set([
    "unauthorized",
    "invalid-request",
    "expired",
    "replayed",
    "overloaded",
    "stale-context",
    "unavailable",
  ]);
  const sameContext = (a, b) =>
    a == null || b == null
      ? a == null && b == null
      : a.projectId === b.projectId &&
        a.worktreeId === b.worktreeId &&
        a.revision === b.revision &&
        a.target?.kind === b.target?.kind &&
        (a.target?.kind !== "wsl" || a.target?.distroId === b.target?.distroId);
  const record = (project) => {
    try {
      if (records.length < 16) records.push(project());
    } catch {
      /* Observations cannot alter invoke settlement. */
    }
  };
  const wrapper = async function (...args) {
    const [command, input] = args;
    const selected = command === "plugin:suite|connection" && input?.request?.method?.kind === "openReceivedFile";
    if (!selected) return Reflect.apply(original, this, args);
    const header = input.request.header;
    try {
      const response = await Reflect.apply(original, this, args);
      record(() => {
        const operation = response?.operation;
        const provenance = operation?.provenance;
        const outcome = operation?.outcome;
        return {
          result: "returned",
          state: states.has(outcome?.state) ? outcome.state : "unknown",
          code: codes.has(outcome?.code) ? outcome.code : null,
          provenanceValid:
            !!provenance &&
            Object.keys(provenance).sort().join(",") === "component,product,requestId,revision" &&
            provenance.product === "workspace" &&
            provenance.component === "workspace.commands" &&
            provenance.requestId === header.requestId &&
            provenance.revision === revision,
          pathShapeValid: typeof response?.value?.path === "string" && response.value.path.length <= 32768,
          contextMatchesRequest: sameContext(response?.value?.context, header.context),
        };
      });
      return response;
    } catch (error) {
      record(() => ({ result: "rejected", code: codes.has(error?.code) ? error.code : "unknown" }));
      throw error;
    }
  };
  bridge.invoke = wrapper;
  window.__devboxFileIpcDiagnostic = {
    records,
    restore() {
      if (bridge.invoke === wrapper) bridge.invoke = original;
      delete window.__devboxFileIpcDiagnostic;
    },
  };
}
