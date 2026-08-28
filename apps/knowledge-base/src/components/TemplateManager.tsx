import { useEffect, useRef, useState } from "react";
import {
  createTemplate,
  deleteTemplate,
  discardTemplatePreview,
  listTemplates,
  previewTemplate,
  saveTemplate,
  updateTemplate,
  type NoteTemplate,
  type TemplatePreview,
  type SaveTemplateResult,
} from "../api";

interface TemplateManagerProps {
  onClose: () => void;
  onSaved?: (result: SaveTemplateResult) => void;
}

const today = new Date();
const defaultDate = String(today.getFullYear()).padStart(4, "0") + "-"
  + String(today.getMonth() + 1).padStart(2, "0") + "-"
  + String(today.getDate()).padStart(2, "0");
const defaultTime = String(today.getHours()).padStart(2, "0") + ":"
  + String(today.getMinutes()).padStart(2, "0");

export default function TemplateManager({ onClose, onSaved }: TemplateManagerProps) {
  const [templates, setTemplates] = useState<NoteTemplate[]>([]);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [name, setName] = useState("");
  const [body, setBody] = useState("# {{title}}\n\nCreated on {{date}} at {{time}}.\n");
  const [target, setTarget] = useState("Notes/new-note.md");
  const [title, setTitle] = useState("New note");
  const [date, setDate] = useState(defaultDate);
  const [time, setTime] = useState(defaultTime);
  const [preview, setPreview] = useState<TemplatePreview | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mountedRef = useRef(true);
  const busyRef = useRef(false);
  const savingRef = useRef(false);
  const previewRef = useRef<TemplatePreview | null>(null);
  const requestRef = useRef(0);
  const dialogRef = useRef<HTMLDivElement | null>(null);
  const previewDialogRef = useRef<HTMLElement | null>(null);
  const restoreFocusRef = useRef<HTMLElement | null>(null);
  const selected = templates.find((template) => template.id === selectedId) ?? null;
  const definitionDirty = selectedId == null
    || selected == null
    || selected.name !== name
    || selected.content !== body;

  previewRef.current = preview;
  busyRef.current = busy;

  useEffect(() => {
    mountedRef.current = true;
    restoreFocusRef.current = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    return () => {
      mountedRef.current = false;
      requestRef.current += 1;
      const current = previewRef.current;
      if (current && !savingRef.current) {
        void discardTemplatePreview(current.previewId).catch(() => undefined);
      }
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
    return () => { active = false; };
  }, []);

  const select = (template: NoteTemplate) => {
    if (busyRef.current || previewRef.current) return;
    requestRef.current += 1;
    setSelectedId(template.id);
    setName(template.name);
    setBody(template.content);
    setError(null);
  };

  const clearEditor = () => {
    if (busyRef.current || previewRef.current) return;
    requestRef.current += 1;
    setSelectedId(null);
    setName("");
    setBody("# {{title}}\n\nCreated on {{date}} at {{time}}.\n");
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
      const saved = selectedId == null
        ? await createTemplate({ name, content: body })
        : await updateTemplate(selectedId, { name, content: body });
      if (!mountedRef.current || requestRef.current !== request) return;
      setTemplates((items) => editingId == null
        ? [...items, saved]
        : items.map((item) => item.id === saved.id ? saved : item));
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
        setBody("# {{title}}\n\nCreated on {{date}} at {{time}}.\n");
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

  const openPreview = async () => {
    if (busy || selectedId == null) return;
    const request = requestRef.current + 1;
    requestRef.current = request;
    const input = {
      templateId: selectedId,
      target,
      title,
      date,
      time,
    };
    setBusy(true);
    busyRef.current = true;
    setError(null);
    try {
      const next = await previewTemplate(input);
      if (!mountedRef.current || requestRef.current !== request) {
        void discardTemplatePreview(next.previewId).catch(() => undefined);
        return;
      }
      setPreview(next);
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

  const cancelPreview = async () => {
    const current = previewRef.current;
    if (!current || busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    try {
      await discardTemplatePreview(current.previewId);
      if (mountedRef.current && previewRef.current?.previewId === current.previewId) {
        setPreview(null);
      }
    } catch (cause) {
      if (mountedRef.current) {
        setError(cause instanceof Error ? cause.message : String(cause));
      }
    } finally {
      busyRef.current = false;
      if (mountedRef.current) setBusy(false);
    }
  };

  const confirmPreview = async () => {
    const current = previewRef.current;
    if (!current || busy) return;
    savingRef.current = true;
    setBusy(true);
    busyRef.current = true;
    setError(null);
    try {
      const result = await saveTemplate(current.previewId);
      if (!mountedRef.current) return;
      setPreview(null);
      onSaved?.(result);
    } catch (cause) {
      // Native save consumes the one-shot approval before any stale-vault,
      // publication, or index failure is returned. Require a fresh preview
      // instead of leaving a UI card that can no longer be saved.
      if (mountedRef.current) {
        setPreview(null);
        setError(cause instanceof Error ? cause.message : String(cause));
      }
    } finally {
      savingRef.current = false;
      busyRef.current = false;
      if (mountedRef.current) setBusy(false);
    }
  };

  const closeManager = async () => {
    const current = previewRef.current;
    if (busyRef.current) return;
    if (current) {
      busyRef.current = true;
      setBusy(true);
      try {
        await discardTemplatePreview(current.previewId);
        if (mountedRef.current && previewRef.current?.previewId === current.previewId) {
          setPreview(null);
        }
      } catch (cause) {
        if (mountedRef.current) {
          setError(cause instanceof Error ? cause.message : String(cause));
          setBusy(false);
          busyRef.current = false;
        }
        return;
      }
      busyRef.current = false;
      if (mountedRef.current) setBusy(false);
    }
    if (mountedRef.current) onClose();
  };

  useEffect(() => {
    const container = (preview ? previewDialogRef.current : dialogRef.current);
    if (!container) return undefined;
    const focusable = () => Array.from(container.querySelectorAll<HTMLElement>(
      "button:not([disabled]), input:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])",
    ));
    const focusTask = window.setTimeout(() => focusable()[0]?.focus(), 0);
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        if (!busyRef.current) {
          event.preventDefault();
          if (previewRef.current) void cancelPreview();
          else void closeManager();
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
  }, [preview]);

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
            <h2 id="template-manager-title">Note templates</h2>
            <p className="dim" id="template-manager-description">Local only · preview is required before a new file is created.</p>
          </div>
          <button className="btn small" type="button" onClick={() => void closeManager()} disabled={busy || Boolean(preview)}>Close</button>
        </div>
        <div className="template-layout">
          <aside className="template-list" aria-label="Saved note templates">
            {templates.map((template) => (
              <button
                type="button"
                className={"template-list-item " + (template.id === selectedId ? "active" : "")}
                key={template.id}
                onClick={() => select(template)}
                disabled={busy || Boolean(preview)}
              >
                <span>{template.name}</span>
                <span className="dim">#{template.id}</span>
              </button>
            ))}
            {templates.length === 0 && <div className="dim">No templates yet.</div>}
            <button className="btn small" type="button" onClick={clearEditor} disabled={busy || Boolean(preview)}>New template</button>
          </aside>
          <section className="template-editor">
            <label>
              Name
              <input value={name} onChange={(event) => setName(event.currentTarget.value)} maxLength={128} disabled={busy || Boolean(preview)} />
            </label>
            <label>
              Markdown
              <textarea value={body} onChange={(event) => setBody(event.currentTarget.value)} rows={9} disabled={busy || Boolean(preview)} />
            </label>
            <div className="dim template-help">Supported variables: <code>{"{{title}}"}</code> <code>{"{{date}}"}</code> <code>{"{{time}}"}</code> <code>{"{{vault-relative-path}}"}</code></div>
            <div className="template-actions">
              <button className="btn" type="button" onClick={() => void saveDefinition()} disabled={busy || !name.trim()}>Save template</button>
              <button className="btn" type="button" onClick={() => void removeDefinition()} disabled={busy || selectedId == null}>Delete</button>
            </div>
            <hr />
            <h3>Apply to a new note</h3>
            <div className="template-grid">
              <label>Target path<input value={target} onChange={(event) => setTarget(event.currentTarget.value)} placeholder="Notes/idea.md" disabled={busy || Boolean(preview)} /></label>
              <label>Title<input value={title} onChange={(event) => setTitle(event.currentTarget.value)} disabled={busy || Boolean(preview)} /></label>
              <label>Date<input type="date" value={date} onChange={(event) => setDate(event.currentTarget.value)} disabled={busy || Boolean(preview)} /></label>
              <label>Time<input type="time" value={time} onChange={(event) => setTime(event.currentTarget.value)} disabled={busy || Boolean(preview)} /></label>
            </div>
            {definitionDirty && <div className="dim">Save the template definition before previewing it.</div>}
            <button className="btn active" type="button" onClick={() => void openPreview()} disabled={busy || selectedId == null || definitionDirty}>Preview before apply</button>
            {error && <div className="source-error" role="alert">{error}</div>}
          </section>
        </div>
      </div>
      {preview && (
        <div className="template-preview-overlay">
          <section
            className="template-preview"
            ref={previewDialogRef}
            role="dialog"
            aria-modal="true"
            aria-labelledby="template-preview-title"
            aria-describedby="template-preview-description"
            tabIndex={-1}
          >
            <h3 id="template-preview-title">Preview · {preview.target}</h3>
            <pre>{preview.content}</pre>
            <div className="dim" id="template-preview-description">{preview.byteLength.toLocaleString()} bytes · existing files are never overwritten</div>
            <div className="template-actions">
              <button className="btn" type="button" onClick={() => void cancelPreview()} disabled={busy}>Cancel</button>
              <button className="btn active" type="button" onClick={() => void confirmPreview()} disabled={busy}>Create note</button>
            </div>
          </section>
        </div>
      )}
    </div>
  );
}
