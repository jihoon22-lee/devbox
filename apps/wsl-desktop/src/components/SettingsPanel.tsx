import { useRef } from "react";
import { restoreFocus, trapDialogKeyDown } from "@devbox/a11y";
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
}

export default function SettingsPanel({ open, settings, onChange, onClose }: SettingsPanelProps) {
  const dialogRef = useRef<HTMLElement>(null);
  const openerRef = useRef<HTMLElement | null>(null);
  if (open && !openerRef.current) {
    openerRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  }

  if (!open) return null;

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
          <legend>터미널 표시</legend>
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
          <button type="button" className="btn primary" onClick={close}>닫기</button>
        </div>
      </section>
    </div>
  );
}
