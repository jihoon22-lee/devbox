import { useRef } from "react";
import { restoreFocus, trapDialogKeyDown } from "@devbox/a11y";
import {
  DEFAULT_TERMINAL_FONT_SIZE,
  MAX_TERMINAL_FONT_SIZE,
  MIN_TERMINAL_FONT_SIZE,
  clampTerminalFontSize,
} from "../lib/terminalUx";
import type { MultiplexerAvailability } from "../types";
import {
  CURSOR_LABELS,
  FONT_CHOICES,
  MAX_SCROLLBACK_LINES,
  MIN_SCROLLBACK_LINES,
  THEME_LABELS,
  clampScrollbackLines,
  type CursorStyle,
  type TerminalSettings,
  type TerminalThemeName,
} from "../lib/settings";

interface SettingsPanelProps {
  open: boolean;
  settings: TerminalSettings;
  onChange: (patch: Partial<TerminalSettings>) => void;
  onClose: () => void;
  muxAvailability: readonly MultiplexerAvailability[];
  muxScanning: boolean;
  onRefreshMux: () => void;
  copyOnSelect: boolean;
  onCopyOnSelectChange: (enabled: boolean) => void;
  fontSize: number;
  onFontSizeChange: (fontSize: number) => void;
}

const MULTIPLEXER_STATUS_SUFFIX: Readonly<Record<MultiplexerAvailability["status"], string>> = {
  available: " (설치됨)",
  missing: " (없음)",
  error: " (확인 오류)",
};

export default function SettingsPanel({
  open,
  settings,
  onChange,
  onClose,
  muxAvailability,
  muxScanning,
  onRefreshMux,
  copyOnSelect,
  onCopyOnSelectChange,
  fontSize,
  onFontSizeChange,
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
