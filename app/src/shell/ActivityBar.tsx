import { useNavigate, useRouterState } from "@tanstack/react-router";
import { Settings } from "lucide-react";
import { IconButton } from "../ui";
import { SECTIONS } from "./sections";

export function ActivityBar() {
  const navigate = useNavigate();
  const path = useRouterState({ select: (s) => s.location.pathname });
  return (
    <nav
      aria-label="섹션"
      style={{
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        gap: 4,
        padding: "8px 0",
        background: "var(--bg-0)",
        borderRight: "1px solid var(--border)",
      }}
    >
      {SECTIONS.map((s) => (
        <IconButton
          key={s.path}
          label={s.label}
          icon={s.icon}
          shortcut={["Ctrl", String(s.digit)]}
          pressed={path.startsWith(s.path)}
          onClick={() => void navigate({ to: s.path })}
        />
      ))}
      <span style={{ flex: 1 }} />
      <IconButton
        label="설정"
        icon={<Settings size={20} />}
        pressed={path.startsWith("/settings")}
        onClick={() => void navigate({ to: "/settings" })}
      />
    </nav>
  );
}
