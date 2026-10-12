import type { ReactNode } from "react";

export function EmptyState({ title, body, action }: { title: string; body?: string; action?: ReactNode }) {
  return (
    <div style={{ display: "grid", placeItems: "center", height: "100%", padding: "var(--space-8)" }}>
      <div style={{ textAlign: "center", maxWidth: 420 }}>
        <p style={{ fontSize: "var(--text-lg)", margin: 0 }}>{title}</p>
        {body && <p style={{ color: "var(--fg-muted)" }}>{body}</p>}
        {action}
      </div>
    </div>
  );
}
