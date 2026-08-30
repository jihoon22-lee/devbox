import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useRef, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent } from "react";
import { isImeComposing } from "@devbox/a11y";
import { CLIPBOARD_PREVIEW_ID, clearRecents, getShortcut, launchResult, performTextAction, previewTextAction, readCurrentText, search, setFavorite, setShortcut } from "./api";
import { isTauri } from "./lib/isTauri";
import type { SearchResponse, SearchResult, ShortcutConfig, ShortcutStatus, SourceDiagnostic } from "./types";
import "./App.css";

const DEFAULT_RESPONSE: SearchResponse = { results: [], sources: [] };
const MAX_HANDOFF_TEXT_BYTES = 64 * 1024;

const SOURCE_STATUS_COPY: Record<SourceDiagnostic["status"], { label: string; description: string }> = {
  fresh: { label: "최신", description: "정상적으로 읽었습니다." },
  stale: { label: "오래됨", description: "마지막 갱신 이후 시간이 지났습니다." },
  missing: { label: "없음", description: "아직 사용할 수 있는 snapshot이 없습니다." },
  corrupt: { label: "손상됨", description: "안전하지 않아 검색에서 제외했습니다." },
  permission: { label: "권한 없음", description: "읽을 권한이 없어 확인하지 못했습니다." },
  linked: { label: "안전하지 않은 링크", description: "symbolic link 또는 reparse point 경로라 제외했습니다." },
};

const SOURCE_NAMES: Record<string, string> = {
  "workbench": "Workbench",
  "repo-manager": "Repo Manager",
  "run-manager": "Run Manager",
  "everything-plus": "Everything+",
  "wsl-desktop": "WSL Desktop",
};

const SOURCE_VIEW_NAMES: Record<string, string> = {
  "profiles": "프로필",
  "repositories": "저장소",
  "jobs-services": "작업·서비스",
  "saved-queries": "저장된 검색",
};

function utf8ByteLength(value: string): number {
  let bytes = 0;
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code <= 0x7f) bytes += 1;
    else if (code <= 0x7ff) bytes += 2;
    else if (code >= 0xd800 && code <= 0xdbff && value.charCodeAt(index + 1) >= 0xdc00 && value.charCodeAt(index + 1) <= 0xdfff) {
      bytes += 4;
      index += 1;
    } else bytes += 3;
  }
  return bytes;
}

function safeError(): string {
  return "Launcher 동작을 완료하지 못했습니다.";
}

async function hideWindow(): Promise<void> {
  if (isTauri()) await getCurrentWindow().hide();
}

export default function App() {
  const [query, setQuery] = useState("");
  const [response, setResponse] = useState(DEFAULT_RESPONSE);
  const [selected, setSelected] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<{ result: SearchResult; text: string; kind: string } | null>(null);
  const [staleResult, setStaleResult] = useState<SearchResult | null>(null);
  const [shortcut, setShortcutState] = useState<ShortcutStatus | null>(null);
  const [shortcutBusy, setShortcutBusy] = useState(false);
  const [favoriteBusyId, setFavoriteBusyId] = useState<string | null>(null);
  const [recentsBusy, setRecentsBusy] = useState(false);
  const requestId = useRef(0);
  const mounted = useRef(true);
  const inputRef = useRef<HTMLInputElement>(null);
  const previewCancelRef = useRef<HTMLButtonElement>(null);
  const staleCancelRef = useRef<HTMLButtonElement>(null);
  const lastDialog = useRef<"preview" | "stale" | null>(null);
  const composition = useRef(false);

  const refresh = useCallback(async (value: string) => {
    const id = ++requestId.current;
    setBusy(true);
    setError(null);
    try {
      const next = await search(value);
      if (mounted.current && id === requestId.current) {
        setResponse(next);
        setSelected(0);
      }
    } catch {
      if (mounted.current && id === requestId.current) {
        setResponse(DEFAULT_RESPONSE);
        setError(safeError());
      }
    } finally {
      if (mounted.current && id === requestId.current) setBusy(false);
    }
  }, []);

  useEffect(() => {
    mounted.current = true;
    void refresh("");
    void getShortcut().then((config) => { if (mounted.current) setShortcutState(config); }).catch(() => undefined);
    inputRef.current?.focus();
    return () => { mounted.current = false; requestId.current += 1; };
  }, [refresh]);

  useEffect(() => {
    const dialog = preview ? "preview" : staleResult ? "stale" : null;
    if (dialog === "preview" && !busy) {
      previewCancelRef.current?.focus();
    } else if (dialog === "stale" && !busy) {
      staleCancelRef.current?.focus();
    } else if (!dialog && lastDialog.current && mounted.current && !busy) {
      inputRef.current?.focus();
    }
    lastDialog.current = dialog;
  }, [busy, preview, staleResult]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.isComposing || event.keyCode === 229 || composition.current) return;
      if (event.key === "Escape") {
        event.preventDefault();
        if (preview) setPreview(null);
        else if (staleResult) setStaleResult(null);
        else void hideWindow();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [preview, staleResult]);

  const executeResult = async (result: SearchResult, allowStale = false) => {
    setError(null);
    if (result.explicitPreview) {
      setBusy(true);
      try {
        // Authorize the action before touching selection/clipboard data. The
        // renderer may request a preview only for a result revalidated by the
        // Rust command boundary.
        const meta = await previewTextAction(result);
        const text = await readCurrentText();
        const isClipboardPreview = result.id === CLIPBOARD_PREVIEW_ID;
        if (isClipboardPreview !== (meta.kind === "clipboard-preview/v1")) throw new Error("preview kind mismatch");
        if (!text) throw new Error("empty");
        if (utf8ByteLength(text) > Math.min(meta.maxBytes, MAX_HANDOFF_TEXT_BYTES)) throw new Error("too large");
        if (mounted.current) setPreview({ result, text, kind: meta.kind });
      } catch {
        if (mounted.current) setError("선택한 텍스트 또는 클립보드를 읽지 못했습니다.");
      } finally { if (mounted.current) setBusy(false); }
      return;
    }
    setBusy(true);
    try {
      const outcome = await launchResult(result, allowStale);
      if (outcome.status === "installRequired" && mounted.current) setError("대상 앱이 없어 Devbox Manager 설치 화면을 열었습니다.");
      else {
        void refresh(query);
        await hideWindow();
      }
    } catch { if (mounted.current) setError(safeError()); }
    finally { if (mounted.current) setBusy(false); }
  };

  const runSelected = async (index = selected) => {
    const result = response.results[index];
    if (!result || busy) return;
    if (result.stale) {
      setStaleResult(result);
      return;
    }
    await executeResult(result);
  };

  const confirmStale = async () => {
    if (!staleResult || busy) return;
    const result = staleResult;
    setStaleResult(null);
    await executeResult(result, true);
  };

  const toggleFavorite = async (result: SearchResult) => {
    if (busy || favoriteBusyId) return;
    setFavoriteBusyId(result.id);
    setBusy(true);
    setError(null);
    try {
      await setFavorite(result, !result.favorite);
      await refresh(query);
    } catch {
      if (mounted.current) setError(safeError());
    } finally {
      if (mounted.current) {
        setFavoriteBusyId(null);
        setBusy(false);
      }
    }
  };

  const clearRecentHistory = async () => {
    if (busy || recentsBusy) return;
    setRecentsBusy(true);
    setBusy(true);
    setError(null);
    try {
      await clearRecents();
      await refresh(query);
    } catch {
      if (mounted.current) setError(safeError());
    } finally {
      if (mounted.current) {
        setRecentsBusy(false);
        setBusy(false);
      }
    }
  };

  const handleDialogKeyDown = (event: ReactKeyboardEvent<HTMLElement>) => {
    if (composition.current || isImeComposing(event)) return;
    if (event.key === "Escape") {
      event.preventDefault();
      if (preview) setPreview(null);
      else if (staleResult) setStaleResult(null);
      return;
    }
    if (event.key !== "Tab") return;
    const focusable = Array.from(event.currentTarget.querySelectorAll<HTMLElement>(
      "button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])",
    ));
    if (focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  const confirmPreview = async () => {
    if (!preview || busy) return;
    if (preview.kind === "clipboard-preview/v1") {
      setPreview(null);
      return;
    }
    setBusy(true);
    try {
      const outcome = await performTextAction(preview.result, preview.text);
      setPreview(null);
      if (outcome.status === "installRequired") setError("대상 앱이 없어 Devbox Manager 설치 화면을 열었습니다.");
      else {
        void refresh(query);
        await hideWindow();
      }
    } catch { if (mounted.current) setError(safeError()); }
    finally { if (mounted.current) setBusy(false); }
  };

  const saveShortcut = async (value: ShortcutConfig["accelerator"]) => {
    const next = { accelerator: value, enabled: true } as ShortcutConfig;
    setShortcutBusy(true);
    try { setShortcutState(await setShortcut(next)); } catch { setError(safeError()); }
    finally { if (mounted.current) setShortcutBusy(false); }
  };

  return (
    <main className="launcher-shell">
      <section className="launcher-panel" aria-labelledby="launcher-title">
        <header className="launcher-header">
          <div>
            <h1 id="launcher-title">Devbox Launcher</h1>
            <p>앱과 검증된 snapshot을 빠르게 엽니다. 클립보드는 명시적 미리보기에서만 읽습니다.</p>
          </div>
          <button type="button" className="icon-button" aria-label="Launcher 닫기" onClick={() => void hideWindow()}>×</button>
        </header>
        <label className="search-label" htmlFor="launcher-search">열기 또는 검색</label>
        <input
          ref={inputRef}
          id="launcher-search"
          className="search-input"
          value={query}
          maxLength={512}
          role="combobox"
          aria-autocomplete="list"
          aria-expanded={response.results.length > 0}
          aria-haspopup="listbox"
          autoComplete="off"
          spellCheck={false}
          aria-controls="launcher-results"
          aria-activedescendant={response.results[selected] ? `result-${selected}` : undefined}
          onCompositionStart={() => { composition.current = true; }}
          onCompositionEnd={(event) => { composition.current = false; setQuery(event.currentTarget.value); void refresh(event.currentTarget.value); }}
          onChange={(event) => { setQuery(event.target.value); if (!composition.current) void refresh(event.target.value); }}
          onKeyDown={(event) => {
            if (event.nativeEvent.isComposing || composition.current || event.keyCode === 229) return;
            if (event.key === "ArrowDown") { event.preventDefault(); setSelected((value) => response.results.length === 0 ? 0 : Math.min(value + 1, response.results.length - 1)); }
            if (event.key === "ArrowUp") { event.preventDefault(); setSelected((value) => Math.max(value - 1, 0)); }
            if (event.key === "Enter") { event.preventDefault(); void runSelected(); }
          }}
        />
        <div className="status" aria-live="polite" aria-atomic="true">{busy ? "확인 중…" : error ?? `${response.results.length}개 결과`}</div>
        <ul id="launcher-results" className="result-list" role="listbox" aria-label="Launcher 결과" aria-busy={busy}>
          {response.results.map((result, index) => (
            <li key={result.id} role="presentation">
              <button id={`result-${index}`} type="button" role="option" aria-selected={selected === index} className={`result ${selected === index ? "selected" : ""}`} onMouseEnter={() => setSelected(index)} onClick={() => { setSelected(index); void runSelected(index); }}>
                <span className="result-heading">
                  <span className="result-label">{result.label}</span>
                  <span className="result-badges" aria-label="결과 상태">
                    {result.favorite && <span className="result-badge favorite-badge">즐겨찾기</span>}
                    {result.recent && <span className="result-badge recent-badge">최근</span>}
                  </span>
                </span>
                <span className="result-detail">{result.detail ?? result.source}{result.stale ? " · 오래됨" : ""}{result.explicitPreview ? " · 미리보기 필요" : ""}</span>
              </button>
            </li>
          ))}
          {!busy && response.results.length === 0 && <li className="empty">일치하는 결과가 없습니다.</li>}
        </ul>
        <footer className="launcher-footer">
          <span>↑↓ 선택 · Enter 열기 · Esc 닫기</span>
          <div className="footer-actions">
            <button
              type="button"
              className="favorite-action"
              aria-label={response.results[selected] ? (response.results[selected].favorite ? `${response.results[selected].label} 즐겨찾기 해제` : `${response.results[selected].label} 즐겨찾기 추가`) : "선택 결과 즐겨찾기"}
              aria-pressed={response.results[selected]?.favorite ?? false}
              onClick={() => { const result = response.results[selected]; if (result) void toggleFavorite(result); }}
              disabled={busy || favoriteBusyId !== null || !response.results[selected]}
            >{response.results[selected]?.favorite ? "★ 즐겨찾기 해제" : "☆ 즐겨찾기"}</button>
            <button type="button" className="clear-recents" onClick={() => void clearRecentHistory()} disabled={busy || recentsBusy}>최근 기록 초기화</button>
            <label>단축키 <select aria-label="Launcher 단축키" value={shortcut?.accelerator ?? "Ctrl+Alt+Space"} disabled={shortcutBusy} onChange={(event) => void saveShortcut(event.target.value as ShortcutConfig["accelerator"])}><option>Ctrl+Alt+Space</option><option>Ctrl+Alt+L</option><option>Ctrl+Alt+J</option></select><small>즉시 적용</small></label>
          </div>
        </footer>
        {shortcut && shortcut.registration !== "registered" && <p className="shortcut-status" role="status">{shortcut.registration === "unavailable" ? `전역 단축키를 등록하지 못했습니다. ${shortcut.alternatives.join(" 또는 ")} 중 하나를 선택해 다시 시도하세요.` : shortcut.registration === "unsupported" ? "이 환경에서는 전역 단축키를 사용할 수 없습니다. 앱 메뉴나 다시 실행으로 Launcher를 여세요." : shortcut.registration === "pending" ? "전역 단축키를 확인하는 중입니다…" : "전역 단축키가 꺼져 있습니다. 위 선택에서 다시 켤 수 있습니다."}</p>}
        <details className="sources"><summary>snapshot source 상태</summary><ul className="source-list">{response.sources.map((source) => { const status = SOURCE_STATUS_COPY[source.status]; return <li key={`${source.producer}:${source.view}`}><span className="source-name">{SOURCE_NAMES[source.producer] ?? source.producer} · {SOURCE_VIEW_NAMES[source.view] ?? source.view}</span><span className={`source-status source-${source.status}`}>{status.label}</span><small>{status.description}</small></li>; })}</ul></details>
      </section>
      {staleResult && <div className="modal-backdrop" role="presentation"><section className="preview-modal confirmation-modal" role="dialog" aria-modal="true" aria-labelledby="stale-title" aria-describedby="stale-description" tabIndex={-1} onKeyDown={handleDialogKeyDown}>
        <h2 id="stale-title">오래된 snapshot입니다</h2>
        <p id="stale-description">{staleResult.label}의 저장된 상태가 오래되었습니다. 계속하면 현재 catalog와 snapshot을 다시 확인한 뒤 대상 앱에 전달합니다.</p>
        <div className="modal-actions">
          <button ref={staleCancelRef} type="button" onClick={() => setStaleResult(null)} disabled={busy}>취소</button>
          <button type="button" onClick={() => void confirmStale()} disabled={busy}>계속 열기</button>
        </div>
      </section></div>}
      {preview && <div className="modal-backdrop" role="presentation"><section className="preview-modal" role="dialog" aria-modal="true" aria-labelledby="preview-title" aria-describedby="preview-description" tabIndex={-1} onKeyDown={handleDialogKeyDown}>
        <h2 id="preview-title">{preview.kind === "clipboard-preview/v1" ? "클립보드 미리보기" : "텍스트 handoff 미리보기"}</h2>
        <p id="preview-description">{preview.kind === "clipboard-preview/v1" ? "명시적으로 요청한 현재 텍스트만 표시하며 전달하거나 저장하지 않습니다." : "이 내용은 확인 후 한 번만 전달되며 Launcher에 저장하지 않습니다."}</p>
        <pre>{preview.text}</pre>
        <div className="modal-actions">
          <button ref={previewCancelRef} type="button" onClick={() => setPreview(null)} disabled={busy}>{preview.kind === "clipboard-preview/v1" ? "닫기" : "취소"}</button>
          {preview.kind !== "clipboard-preview/v1" && <button type="button" onClick={() => void confirmPreview()} disabled={busy}>전달</button>}
        </div>
      </section></div>}
    </main>
  );
}
