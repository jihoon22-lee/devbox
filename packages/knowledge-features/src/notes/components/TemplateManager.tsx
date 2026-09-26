import { useCallback, useEffect, useRef, useState } from "react";
import { isImeComposing } from "@devbox/a11y";
import {
  createTemplate,
  deleteTemplate,
  listTemplates,
  createNoteFromTemplate,
  updateTemplate,
  type NoteTemplate,
} from "../api";

import type { QuickCaptureSaved } from "../types";

interface TemplateManagerProps {
  active?: boolean;
  onClose: () => void;
  onSaved?: (result: QuickCaptureSaved) => void;
}

const today = new Date();
const defaultDate =
  String(today.getFullYear()).padStart(4, "0") +
  "-" +
  String(today.getMonth() + 1).padStart(2, "0") +
  "-" +
  String(today.getDate()).padStart(2, "0");
const defaultTime = String(today.getHours()).padStart(2, "0") + ":" + String(today.getMinutes()).padStart(2, "0");

export default function TemplateManager({ active = true, onClose, onSaved }: TemplateManagerProps) {
  const [templates, setTemplates] = useState<NoteTemplate[]>([]);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [name, setName] = useState("");
  const [body, setBody] = useState("# {{title}}\n\n{{date}} {{time}}에 작성되었습니다.\n");
  const [target, setTarget] = useState("Notes/new-note.md");
  const [title, setTitle] = useState("새 노트");
  const [date, setDate] = useState(defaultDate);
  const [time, setTime] = useState(defaultTime);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mountedRef = useRef(true);
  const busyRef = useRef(false);
  const requestRef = useRef(0);
  const dialogRef = useRef<HTMLDivElement | null>(null);
  const restoreFocusRef = useRef<HTMLElement | null>(null);
  const selected = templates.find((template) => template.id === selectedId) ?? null;
  const definitionDirty = selectedId == null || selected == null || selected.name !== name || selected.content !== body;

  busyRef.current = busy;

  useEffect(() => {
    mountedRef.current = true;
    restoreFocusRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    return () => {
      mountedRef.current = false;
      requestRef.current += 1;
      const opener = restoreFocusRef.current;
      if (opener && document.contains(opener)) {
        window.setTimeout(() => {
          if (document.contains(opener)) opener.focus();
        }, 0);
      }
    };
  }, []);

  useEffect(() => {
    let active = true;
    void listTemplates()
      .then((items) => {
        if (!active) return;
        setTemplates(items);
        if (items[0]) {
          setSelectedId(items[0].id);
          setName(items[0].name);
          setBody(items[0].content);
        }
      })
      .catch((cause) => active && setError(cause instanceof Error ? cause.message : String(cause)));
    return () => {
      active = false;
    };
  }, []);

  const select = (template: NoteTemplate) => {
    if (busyRef.current) return;
    requestRef.current += 1;
    setSelectedId(template.id);
    setName(template.name);
    setBody(template.content);
    setError(null);
  };

  const clearEditor = () => {
    if (busyRef.current) return;
    requestRef.current += 1;
    setSelectedId(null);
    setName("");
    setBody("# {{title}}\n\n{{date}} {{time}}에 작성되었습니다.\n");
    setError(null);
  };

  const saveDefinition = async () => {
    if (busy) return;
    const request = requestRef.current + 1;
    requestRef.current = request;
    const editingId = selectedId;
    setBusy(true);
    busyRef.current = true;
    setError(null);
    try {
      const saved =
        selectedId == null
          ? await createTemplate({ name, content: body })
          : await updateTemplate(selectedId, { name, content: body });
      if (!mountedRef.current || requestRef.current !== request) return;
      setTemplates((items) =>
        editingId == null ? [...items, saved] : items.map((item) => (item.id === saved.id ? saved : item)),
      );
      setSelectedId(saved.id);
      setName(saved.name);
      setBody(saved.content);
    } catch (cause) {
      if (mountedRef.current) {
        setError(cause instanceof Error ? cause.message : String(cause));
      }
    } finally {
      if (mountedRef.current) {
        busyRef.current = false;
        setBusy(false);
      }
    }
  };

  const removeDefinition = async () => {
    if (selectedId == null || busy) return;
    const deletingId = selectedId;
    const request = requestRef.current + 1;
    requestRef.current = request;
    setBusy(true);
    busyRef.current = true;
    setError(null);
    try {
      await deleteTemplate(deletingId);
      if (!mountedRef.current || requestRef.current !== request) return;
      const remaining = templates.filter((item) => item.id !== deletingId);
      setTemplates(remaining);
      if (remaining[0]) {
        setSelectedId(remaining[0].id);
        setName(remaining[0].name);
        setBody(remaining[0].content);
      } else {
        setSelectedId(null);
        setName("");
        setBody("# {{title}}\n\n{{date}} {{time}}에 작성되었습니다.\n");
      }
    } catch (cause) {
      if (mountedRef.current) {
        setError(cause instanceof Error ? cause.message : String(cause));
      }
    } finally {
      if (mountedRef.current) {
        busyRef.current = false;
        setBusy(false);
      }
    }
  };

  const createNote = async () => {
    if (busyRef.current || selectedId == null || definitionDirty) return;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    try {
      const result = await createNoteFromTemplate({ templateId: selectedId, target, title, date, time });
      if (mountedRef.current) onSaved?.(result);
    } catch (cause) {
      if (mountedRef.current) setError(cause instanceof Error ? cause.message : "노트를 만들지 못했습니다.");
    } finally {
      busyRef.current = false;
      if (mountedRef.current) setBusy(false);
    }
  };
  const closeManager = useCallback(() => {
    if (!busyRef.current && mountedRef.current) onClose();
  }, [onClose]);

  useEffect(() => {
    if (!active) return;
    const container = dialogRef.current;
    if (!container) return undefined;
    const focusable = () =>
      Array.from(
        container.querySelectorAll<HTMLElement>(
          "button:not([disabled]), input:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])",
        ),
      );
    const focusTask = window.setTimeout(() => focusable()[0]?.focus(), 0);
    const onKeyDown = (event: KeyboardEvent) => {
      if (isImeComposing(event)) return;
      if (event.key === "Escape") {
        if (!busyRef.current) {
          event.preventDefault();
          closeManager();
        }
        return;
      }
      if (event.key !== "Tab") return;
      const items = focusable();
      if (items.length === 0) {
        event.preventDefault();
        return;
      }
      const first = items[0];
      const last = items[items.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.clearTimeout(focusTask);
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [active, closeManager]);

  return (
    <div
      className="template-modal"
      ref={dialogRef}
      role="dialog"
      aria-modal="true"
      aria-labelledby="template-manager-title"
      aria-describedby="template-manager-description"
      aria-busy={busy}
      tabIndex={-1}
    >
      <div className="template-dialog">
        <div className="template-dialog-head">
          <div>
            <h2 id="template-manager-title">노트 템플릿</h2>
            <p className="dim" id="template-manager-description">
              로컬 전용 · 새 노트를 만들고 잠시 동안 되돌릴 수 있습니다.
            </p>
          </div>
          <button className="btn small" type="button" onClick={() => void closeManager()} disabled={busy}>
            닫기
          </button>
        </div>
        <div className="template-layout">
          <aside className="template-list" aria-label="저장된 노트 템플릿">
            {templates.map((template) => (
              <button
                type="button"
                className={"template-list-item " + (template.id === selectedId ? "active" : "")}
                key={template.id}
                onClick={() => select(template)}
                disabled={busy}
              >
                <span>{template.name}</span>
                <span className="dim">#{template.id}</span>
              </button>
            ))}
            {templates.length === 0 && <div className="dim">아직 템플릿이 없습니다.</div>}
            <button className="btn small" type="button" onClick={clearEditor} disabled={busy}>
              새 템플릿
            </button>
          </aside>
          <section className="template-editor">
            <label>
              이름
              <input
                value={name}
                onChange={(event) => setName(event.currentTarget.value)}
                maxLength={128}
                disabled={busy}
              />
            </label>
            <label>
              Markdown
              <textarea
                value={body}
                onChange={(event) => setBody(event.currentTarget.value)}
                rows={9}
                disabled={busy}
              />
            </label>
            <div className="dim template-help">
              지원 변수: <code>{"{{title}}"}</code> <code>{"{{date}}"}</code> <code>{"{{time}}"}</code>{" "}
              <code>{"{{vault-relative-path}}"}</code>
            </div>
            <div className="template-actions">
              <button
                className="btn"
                type="button"
                onClick={() => void saveDefinition()}
                disabled={busy || !name.trim()}
              >
                템플릿 저장
              </button>
              <button
                className="btn"
                type="button"
                onClick={() => void removeDefinition()}
                disabled={busy || selectedId == null}
              >
                삭제
              </button>
            </div>
            <hr />
            <h3>새 노트에 적용</h3>
            <div className="template-grid">
              <label>
                대상 경로
                <input
                  value={target}
                  onChange={(event) => setTarget(event.currentTarget.value)}
                  placeholder="Notes/idea.md"
                  disabled={busy}
                />
              </label>
              <label>
                제목
                <input value={title} onChange={(event) => setTitle(event.currentTarget.value)} disabled={busy} />
              </label>
              <label>
                날짜
                <input
                  type="date"
                  value={date}
                  onChange={(event) => setDate(event.currentTarget.value)}
                  disabled={busy}
                />
              </label>
              <label>
                시간
                <input
                  type="time"
                  value={time}
                  onChange={(event) => setTime(event.currentTarget.value)}
                  disabled={busy}
                />
              </label>
            </div>
            {definitionDirty && <div className="dim">노트를 만들기 전에 템플릿 정의를 저장하세요.</div>}
            <button
              className="btn active"
              type="button"
              onClick={() => void createNote()}
              disabled={busy || selectedId == null || definitionDirty}
            >
              노트 만들기
            </button>
            {error && (
              <div className="source-error" role="alert">
                {error}
              </div>
            )}
          </section>
        </div>
      </div>
    </div>
  );
}
