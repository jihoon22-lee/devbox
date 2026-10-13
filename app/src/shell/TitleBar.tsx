import { Search } from "lucide-react";
import type { ReactNode } from "react";
import { NotificationCenter } from "../features/notifications/NotificationCenter";
import { Button, Kbd } from "../ui";

export function TitleBar({ project, onOpenPalette }: { project: ReactNode; onOpenPalette: () => void }) {
  return (
    <header
      style={{
        display: "flex",
        alignItems: "center",
        gap: "var(--space-3)",
        padding: "0 var(--space-3)",
        background: "var(--bg-0)",
        borderBottom: "1px solid var(--border)",
      }}
    >
      <strong style={{ fontSize: "var(--text-sm)" }}>devbox</strong>
      {project}
      <span style={{ flex: 1 }} />
      <NotificationCenter />
      <Button variant="ghost" size="sm" onClick={onOpenPalette} aria-label="명령 팔레트 열기">
        <Search size={14} /> 명령·검색 <Kbd keys={["Ctrl", "Shift", "P"]} />
      </Button>
    </header>
  );
}
