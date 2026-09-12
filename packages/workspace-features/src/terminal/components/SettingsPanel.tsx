import { useRef } from "react";
import { restoreFocus, trapDialogKeyDown } from "@devbox/a11y";
import {
  DEFAULT_TERMINAL_FONT_SIZE,
  MAX_TERMINAL_FONT_SIZE,
  MIN_TERMINAL_FONT_SIZE,
  clampTerminalFontSize,
} from "../lib/terminalUx";
import type { MultiplexerAvailability } from "../types";
import type { QuickSummonStatus } from "../api";
import type { AskDialog } from "./AppDialog";
import ShellIntegrationSettings from "./ShellIntegrationSettings";
import {
  CURSOR_LABELS,
  FONT_CHOICES,
  MAX_SCROLLBACK_LINES,
  MIN_SCROLLBACK_LINES,
  QUICK_SUMMON_SHORTCUTS,
  THEME_LABELS,
  clampScrollbackLines,
  type CursorStyle,
  type TerminalSettings,
  type TerminalThemeName,
} from "../lib/settings";

interface SettingsPanelProps {
  open: boolean;
  settings: TerminalSettings;
  quickSummonStatus: QuickSummonStatus | null;
  onChange: (patch: Partial<TerminalSettings>) => void;
  onClose: () => void;
  muxAvailability: readonly MultiplexerAvailability[];
  muxScanning: boolean;
  onRefreshMux: () => void;
  copyOnSelect: boolean;
  onCopyOnSelectChange: (enabled: boolean) => void;
  fontSize: number;
  onFontSizeChange: (fontSize: number) => void;
  distro: string;
  ask: AskDialog;
  onError: (message: string) => void;
}

const MULTIPLEXER_STATUS_SUFFIX: Readonly<Record<MultiplexerAvailability["status"], string>> = {
  available: " (설치됨)",
  missing: " (없음)",
  error: " (확인 오류)",
};

function shortcutStatusMessage(settings: TerminalSettings, status: QuickSummonStatus | null): string {
  if (!settings.quickSummonEnabled) return "전역 단축키를 사용하지 않습니다.";
  if (!status) return "전역 단축키 등록 상태를 확인하는 중입니다…";
  if (status.issues.includes("backendUnavailable")) {
    return "native 설정에 연결하지 못했습니다. 앱을 다시 시작한 뒤 확인하세요.";
  }
  if (status.issues.includes("invalidShortcut")) {
    return "허용되지 않은 단축키입니다. 목록에서 다시 선택하세요.";
  }
  if (status.issues.includes("shortcutBackendUnavailable")) {
    return "이 환경에서 전역 단축키 기능을 시작하지 못했습니다. WSL Desktop을 다시 시작한 뒤 확인하세요.";
  }
  if (status.issues.includes("shortcutRollbackFailed")) {
    return "단축키 변경과 이전 단축키 복구에 실패했습니다. 다른 조합을 선택하거나 앱을 다시 시작하세요.";
  }
  if (status.issues.includes("shortcutUnavailable")) {
    const retained = status.activeShortcut ? ` 이전 단축키 ${status.activeShortcut}는 계속 동작합니다.` : "";
    return `선택한 단축키를 등록하지 못했습니다. 다른 앱이 사용 중이거나 Windows가 예약한 조합일 수 있습니다.${retained}`;
  }
  if (status.shortcutRegistered) return `등록됨: ${status.activeShortcut ?? settings.quickSummonShortcut}`;
  return "전역 단축키가 등록되지 않았습니다.";
}

export default function SettingsPanel({
  open,
  settings,
  quickSummonStatus,
  onChange,
  onClose,
  muxAvailability,
  muxScanning,
  onRefreshMux,
  copyOnSelect,
  onCopyOnSelectChange,
  fontSize,
  onFontSizeChange,
  distro,
  ask,
  onError,
}: SettingsPanelProps) {
  const dialogRef = useRef<HTMLElement>(null);
  const openerRef = useRef<HTMLElement | null>(null);
  if (open && !openerRef.current) {
    openerRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  }

  if (!open) return null;

  const preferredMux = muxAvailability.find((item) => item.kind === settings.multiplexer);
  const preferredMuxUnavailable = settings.multiplexer !== "native" && preferredMux?.status !== "available";

  const close = () => {
    const opener = openerRef.current;
    openerRef.current = null;
    onClose();
    restoreFocus(opener);
  };

  return (
    <div
      className="dialog-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) close();
      }}
    >
      <section
        ref={dialogRef}
        className="settings-panel"
        role="dialog"
        aria-modal="true"
        aria-label="WSL Desktop 설정"
        onKeyDown={(event) => {
          if (dialogRef.current) trapDialogKeyDown(event, dialogRef.current, close);
        }}
      >
        <h2 className="dialog-title">설정</h2>

        <fieldset className="settings-group">
          <legend>동작</legend>
          <label className="settings-row">
            <input
              type="checkbox"
              checked={settings.confirmSinglePaneClose}
              onChange={(event) => onChange({ confirmSinglePaneClose: event.currentTarget.checked })}
            />
            <span>
              팬 하나를 닫을 때 확인
              <small>탭과 여러 팬을 닫을 때는 이 설정과 무관하게 항상 확인합니다.</small>
            </span>
          </label>
          <label className="settings-row">
            <input
              type="checkbox"
              checked={settings.openTerminalOnStart}
              onChange={(event) => onChange({ openTerminalOnStart: event.currentTarget.checked })}
            />
            <span>
              시작할 때 터미널 하나 열기
              <small>복원할 레이아웃이 없고 배포판 조회에 성공한 경우에만 엽니다.</small>
            </span>
          </label>
        </fieldset>

        <fieldset className="settings-group quick-summon-settings">
          <legend>빠른 호출</legend>
          <label className="settings-row">
            <input
              type="checkbox"
              checked={settings.quickSummonEnabled}
              onChange={(event) => onChange({ quickSummonEnabled: event.currentTarget.checked })}
            />
            <span>
              시스템 전역 단축키로 창 표시·숨기기
              <small>다른 앱을 사용 중이어도 기존 WSL Desktop 창과 터미널을 그대로 호출합니다.</small>
            </span>
          </label>
          <label className="settings-row">
            <span>전역 단축키</span>
            <select
              aria-label="빠른 호출 전역 단축키"
              value={settings.quickSummonShortcut}
              disabled={!settings.quickSummonEnabled}
              onChange={(event) => onChange({
                quickSummonShortcut: event.currentTarget.value as TerminalSettings["quickSummonShortcut"],
              })}
            >
              {QUICK_SUMMON_SHORTCUTS.map((choice) => (
                <option key={choice.value} value={choice.value}>{choice.label}</option>
              ))}
            </select>
          </label>
          <p
            className={`quick-summon-status ${quickSummonStatus?.issues.length ? "warning" : ""}`}
            role="status"
            aria-live="polite"
          >
            {shortcutStatusMessage(settings, quickSummonStatus)}
          </p>
          <label className="settings-row">
            <input
              type="checkbox"
              checked={settings.keepInTray}
              onChange={(event) => onChange({ keepInTray: event.currentTarget.checked })}
            />
            <span>
              닫을 때 트레이에 유지
              <small>트레이 메뉴의 ‘완전히 종료’를 선택할 때만 프로세스와 native PTY가 종료됩니다.</small>
            </span>
          </label>
          <div
            className={`quick-summon-close-behavior ${quickSummonStatus?.issues.includes("trayUnavailable") ? "warning" : ""}`}
            role="status"
          >
            {settings.keepInTray && !quickSummonStatus
              ? "닫기 버튼 동작을 적용하는 중입니다…"
              : quickSummonStatus?.issues.includes("trayUnavailable")
                ? "트레이를 만들지 못했습니다. 안전을 위해 닫기 버튼은 앱을 종료합니다."
                : quickSummonStatus?.closeBehavior === "hideToTray"
                  ? "현재 닫기 버튼: 창만 숨기고 터미널 상태 유지"
                  : "현재 닫기 버튼: 앱과 native PTY 세션 종료"}
          </div>
        </fieldset>

        <ShellIntegrationSettings distro={distro} ask={ask} onError={onError} />

        <fieldset className="settings-group">
          <legend>세션</legend>
          <label className="settings-row">
            <span>
              세션 유지 방식
              <small>native는 외부 도구 없이 동작합니다. tmux·zellij는 설치된 배포판에서만 고를 수 있습니다.</small>
            </span>
            <select
              aria-label="세션 유지 방식"
              value={settings.multiplexer}
              onChange={(event) => onChange({ multiplexer: event.currentTarget.value as TerminalSettings["multiplexer"] })}
            >
              {muxAvailability.map((item) => (
                <option key={item.kind} value={item.kind} disabled={item.status !== "available"}>
                  {item.kind}{item.kind === "native" ? " (기본)" : MULTIPLEXER_STATUS_SUFFIX[item.status]}
                </option>
              ))}
            </select>
          </label>
          {preferredMuxUnavailable && (
            <div className="banner" role="status">
              선호 방식은 {settings.multiplexer}로 유지됩니다. 지금 사용할 수 없으면 새 터미널만 native로 열립니다.
            </div>
          )}
          <div className="settings-row">
            <span>
              설치 상태 다시 확인
              <small>선택한 배포판의 PATH와 사용자 설치 경로를 다시 검색합니다.</small>
            </span>
            <button
              type="button"
              className="btn compact"
              disabled={muxScanning}
              aria-busy={muxScanning}
              onClick={onRefreshMux}
            >
              {muxScanning ? "검색 중…" : "다시 검색"}
            </button>
          </div>
          <label className="settings-row">
            <input
              type="checkbox"
              checked={copyOnSelect}
              onChange={(event) => onCopyOnSelectChange(event.currentTarget.checked)}
            />
            <span>선택한 터미널 텍스트를 자동으로 복사</span>
          </label>
        </fieldset>

        <fieldset className="settings-group">
          <legend>터미널 표시</legend>
          <label className="settings-row">
            <span>
              글꼴 크기
              <small>터미널에서 Ctrl 그리고 +, -, 0으로도 바꿀 수 있습니다.</small>
            </span>
            <input
              type="number"
              aria-label="터미널 글꼴 크기"
              min={MIN_TERMINAL_FONT_SIZE}
              max={MAX_TERMINAL_FONT_SIZE}
              step={1}
              value={fontSize}
              onChange={(event) => onFontSizeChange(clampTerminalFontSize(Number(event.currentTarget.value)))}
            />
          </label>
          <label className="settings-row">
            <span>글꼴</span>
            <select
              aria-label="터미널 글꼴"
              value={settings.fontId}
              onChange={(event) => onChange({ fontId: event.currentTarget.value })}
            >
              {FONT_CHOICES.map((choice) => (
                <option key={choice.id} value={choice.id}>{choice.label}</option>
              ))}
            </select>
          </label>
          <label className="settings-row">
            <span>색 테마</span>
            <select
              aria-label="터미널 색 테마"
              value={settings.theme}
              onChange={(event) => onChange({ theme: event.currentTarget.value as TerminalThemeName })}
            >
              {(Object.keys(THEME_LABELS) as TerminalThemeName[]).map((name) => (
                <option key={name} value={name}>{THEME_LABELS[name]}</option>
              ))}
            </select>
          </label>
          <label className="settings-row">
            <span>커서 모양</span>
            <select
              aria-label="터미널 커서 모양"
              value={settings.cursorStyle}
              onChange={(event) => onChange({ cursorStyle: event.currentTarget.value as CursorStyle })}
            >
              {(Object.keys(CURSOR_LABELS) as CursorStyle[]).map((style) => (
                <option key={style} value={style}>{CURSOR_LABELS[style]}</option>
              ))}
            </select>
          </label>
          <label className="settings-row">
            <input
              type="checkbox"
              checked={settings.cursorBlink}
              onChange={(event) => onChange({ cursorBlink: event.currentTarget.checked })}
            />
            <span>커서 깜박임</span>
          </label>
          <label className="settings-row">
            <span>스크롤백 줄 수</span>
            <input
              type="number"
              aria-label="스크롤백 줄 수"
              min={MIN_SCROLLBACK_LINES}
              max={MAX_SCROLLBACK_LINES}
              step={1000}
              value={settings.scrollbackLines}
              onChange={(event) => onChange({ scrollbackLines: clampScrollbackLines(Number(event.currentTarget.value)) })}
            />
          </label>
        </fieldset>

        <div className="dialog-actions">
          <button type="button" className="btn" onClick={() => onFontSizeChange(DEFAULT_TERMINAL_FONT_SIZE)}>
            글꼴 크기 초기화
          </button>
          <button type="button" className="btn primary" onClick={close}>닫기</button>
        </div>
      </section>
    </div>
  );
}
