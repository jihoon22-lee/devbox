import { useNavigate, useRouterState } from "@tanstack/react-router";
import { Settings } from "lucide-react";
import { useAgentSummary } from "../features/agents/AgentData";
import { Badge, IconButton } from "../ui";
import { SECTIONS } from "./sections";

export function ActivityBar() {
  const navigate = useNavigate();
  const agents = useAgentSummary();
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
        <div key={s.path} style={{ position: "relative" }}>
          <IconButton
            label={s.label}
            icon={s.icon}
            shortcut={["Ctrl", String(s.digit)]}
            pressed={path.startsWith(s.path)}
            onClick={() => void navigate({ to: s.path })}
          />
          {s.path === "/agents" && !!agents?.attention && (
            <Badge
              aria-label={`주의가 필요한 에이전트 작업 ${agents.attention}개`}
              tone={agents.failed ? "danger" : "warn"}
              style={{ position: "absolute", right: -4, top: -2, pointerEvents: "none" }}
            >
              {agents.attention}
            </Badge>
          )}
        </div>
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
