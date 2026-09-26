import type { Screen } from "../lib/taskViewTypes";
import { hideMainWindow, quitApp } from "../api";
import type * as React from "react";

interface Props {
  screen: Screen;
  setScreen: React.Dispatch<React.SetStateAction<Screen>>;
  jobs: import("../types").Job[];
  services: import("../types").Job[];
  busy: boolean;
  startupStatus: import("../types").StartupShortcutStatus | null;
  toggleStartup: () => Promise<void>;
}

export function TaskSidebar({ screen, setScreen, jobs, services, busy, startupStatus, toggleStartup }: Props) {
  return (
    <aside className="sidebar">
      <div className="brand-mark" aria-hidden="true">
        RM
      </div>
      <div>
        <h1>Run Manager</h1>
        <p>작업과 서비스를 한곳에서 관리합니다.</p>
      </div>
      <nav aria-label="주요 화면">
        <button
          className={`nav-item ${screen === "jobs" || screen === "editor" ? "active" : ""}`}
          type="button"
          onClick={() => setScreen("jobs")}
        >
          작업 <span>{jobs.length}</span>
        </button>
        <button
          className={`nav-item ${screen === "services" || screen === "service-editor" ? "active" : ""}`}
          type="button"
          onClick={() => setScreen("services")}
        >
          서비스 <span>{services.length}</span>
        </button>
        <button
          className={`nav-item ${screen === "history" ? "active" : ""}`}
          type="button"
          onClick={() => setScreen("history")}
        >
          실행 기록
        </button>
      </nav>
      <div className="sidebar-actions">
        <button
          type="button"
          disabled={busy || !startupStatus?.supported}
          title={startupStatus?.shortcutPath}
          onClick={() => void toggleStartup()}
        >
          {startupStatus?.supported
            ? `로그인 시 자동 시작: ${startupStatus.enabled ? "켜짐" : "꺼짐"}`
            : "자동 시작: Windows 전용"}
        </button>
        <button type="button" onClick={() => void hideMainWindow()}>
          트레이로 숨기기
        </button>
        <button className="danger" type="button" onClick={() => void quitApp()}>
          안전하게 종료
        </button>
      </div>
    </aside>
  );
}
