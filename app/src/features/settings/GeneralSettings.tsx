import { useEffect, useState } from "react";
import { SECTIONS } from "../../shell/sections";
import { ActivityConsent } from "../activity/Consent";
import { NativeAutostartSettings } from "./NativeSettings";
import { isNative } from "./nativePreferences";
import {
  type StartSection,
  setStartSection,
  setTerminalFont,
  setTheme,
  setUiScale,
  type Theme,
  usePreferences,
} from "./preferences";

function NumberPreference({
  label,
  value,
  min,
  max,
  onCommit,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  onCommit(value: number): void;
}) {
  const [draft, setDraft] = useState(String(value));
  useEffect(() => setDraft(String(value)), [value]);
  const commit = () => {
    const parsed = draft.trim() ? Number(draft) : Number.NaN;
    if (!Number.isFinite(parsed)) {
      setDraft(String(value));
      return;
    }
    const next = Math.min(max, Math.max(min, parsed));
    setDraft(String(next));
    onCommit(next);
  };
  return (
    <p>
      <label>
        {label}{" "}
        <input
          type="number"
          min={min}
          max={max}
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onBlur={commit}
          onKeyDown={(event) => {
            if (event.key === "Enter") event.currentTarget.blur();
          }}
        />
      </label>
    </p>
  );
}

export function GeneralSettings() {
  const preferences = usePreferences();
  return (
    <section aria-label="일반 설정">
      <h3>일반</h3>
      {isNative() && <NativeAutostartSettings />}
      {isNative() && <ActivityConsent />}
      <p>
        <label>
          테마{" "}
          <select value={preferences.theme} onChange={(event) => setTheme(event.target.value as Theme)}>
            <option value="system">시스템</option>
            <option value="dark">다크</option>
            <option value="light">라이트</option>
          </select>
        </label>
      </p>
      <NumberPreference
        label="UI 배율 (%)"
        min={90}
        max={150}
        value={Math.round(preferences.uiScale * 100)}
        onCommit={(value) => setUiScale(value / 100)}
      />
      <NumberPreference
        label="터미널 글꼴 크기"
        min={9}
        max={28}
        value={preferences.terminalFont}
        onCommit={setTerminalFont}
      />
      <p>
        <label>
          시작 섹션{" "}
          <select
            value={preferences.startSection}
            onChange={(event) => setStartSection(event.target.value as StartSection)}
          >
            <option value="last">마지막으로 연 섹션</option>
            {SECTIONS.map((section) => (
              <option key={section.path} value={section.path}>
                {section.label}
              </option>
            ))}
          </select>
        </label>
      </p>
    </section>
  );
}
