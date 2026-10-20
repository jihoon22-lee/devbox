import { Tabs } from "../../ui";
import { AboutSettings } from "./AboutSettings";
import { AgentsSettings } from "./AgentsSettings";
import { DiagnosticsSettings } from "./DiagnosticsSettings";
import { GeneralSettings } from "./GeneralSettings";
import { type SettingsTab, useSettingsUi } from "./open";
import { SecretsSettings } from "./SecretsSettings";
import { ShortcutsSettings } from "./ShortcutsSettings";

const items = [
  { id: "general", label: "일반" },
  { id: "agents", label: "에이전트" },
  { id: "secrets", label: "비밀" },
  { id: "shortcuts", label: "단축키" },
  { id: "diagnostics", label: "앱 진단" },
  { id: "about", label: "정보" },
];
export function SettingsPage() {
  const { tab, setTab } = useSettingsUi();
  return (
    <div style={{ padding: "var(--space-6)" }}>
      <h2>설정</h2>
      <Tabs items={items} active={tab} onSelect={(tab) => setTab(tab as SettingsTab)} label="설정 탭" />
      <div role="tabpanel" aria-label={items.find((item) => item.id === tab)?.label}>
        {tab === "general" && <GeneralSettings />}
        {tab === "agents" && <AgentsSettings />}
        {tab === "secrets" && <SecretsSettings />}
        {tab === "shortcuts" && <ShortcutsSettings />}
        {tab === "diagnostics" && <DiagnosticsSettings />}
        {tab === "about" && <AboutSettings />}
      </div>
    </div>
  );
}
