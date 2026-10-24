import { EnvCheck } from "../tools/env/EnvCheck";
/** Settings and the tools section share this single diagnostics screen. */
export function DiagnosticsSettings() {
  return (
    <div style={{ padding: "var(--space-6)" }}>
      <h3>앱 진단</h3>
      <EnvCheck />
    </div>
  );
}
