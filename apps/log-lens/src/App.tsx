import {
  ContextMenu,
  useContextMenu,
  type ContextMenuEntry,
} from "@devbox/context-menu";
import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent, type ReactNode } from "react";
import { cancelRead, exportRecords, readSources } from "./api";
import { browserSnapshot } from "./browserFixture";
import {
  createLiteralRegex,
  createSafeRegex,
  filterRecords,
  recordKey,
  truncateUtf8,
} from "./filter";
import { isTauri } from "./lib/isTauri";
import type {
  ContainerEngine,
  FileCursor,
  FilterSpec,
  LogLevel,
  LogRecord,
  SourceKind,
  SourceSpec,
  SavedView,
  SourcesSnapshot,
} from "./types";
import "./App.css";

const MAX_RENDERED_ROWS = 2_000;
const MAX_SOURCES = 16;
const MAX_SAVED_VIEWS = 20;
const MAX_HIGHLIGHTS = 256;

function makeOperationId(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) return crypto.randomUUID();
  return `op-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`;
}

function formatTimestamp(timestamp: number | null): string {
  const iso = toIsoTimestamp(timestamp);
  return iso ?? "—";
}

function toIsoTimestamp(timestamp: number | null): string | undefined {
  if (timestamp === null || !Number.isSafeInteger(timestamp)) return undefined;
  const date = new Date(timestamp);
  return Number.isNaN(date.getTime()) ? undefined : date.toISOString();
}

function labelForKind(kind: SourceKind): string {
  switch (kind) {
    case "localFile": return "Local file";
    case "directory": return "Local directory";
    case "wslFile": return "WSL file";
    case "wslJournal": return "WSL journal";
    case "run": return "Run Manager handoff";
    case "container": return "Container logs";
  }
}

function compareOpaqueIds(left: string, right: string): number {
  // Native merge uses Rust's bytewise BTreeMap ordering. `localeCompare`
  // varies by machine locale/options, which could otherwise reorder equal
  // timestamps differently between browser fixtures and the packaged app.
  if (left === right) return 0;
  return left < right ? -1 : 1;
}

function mergeClientRecords(
  existing: LogRecord[],
  incoming: LogRecord[],
  sourceSummaries: SourcesSnapshot["sources"],
  cursors: SourcesSnapshot["cursors"],
  statuses: SourcesSnapshot["statuses"],
): LogRecord[] {
  const byKey = new Map(existing.map((record) => [recordKey(record), record]));
  sourceSummaries.forEach((source, index) => {
    if (!cursors[index] || statuses[index] === "rotated" || statuses[index] === "truncated") {
      for (const [key, record] of byKey) {
        if (record.sourceId === source.sourceId) byKey.delete(key);
      }
    }
  });
  for (const record of incoming) byKey.set(recordKey(record), record);
  return [...byKey.values()]
    .sort((left, right) => compareSafeNumbers(left.timestampMillis, right.timestampMillis)
      || compareOpaqueIds(left.sourceId, right.sourceId)
      || left.sequence - right.sequence)
    .slice(-100_000);
}

function compareSafeNumbers(left: number | null, right: number | null): number {
  const leftValue = left ?? Number.MAX_SAFE_INTEGER;
  const rightValue = right ?? Number.MAX_SAFE_INTEGER;
  if (leftValue === rightValue) return 0;
  return leftValue < rightValue ? -1 : 1;
}

function highlightMessage(message: string, filter: FilterSpec): ReactNode {
  if (!filter.text) return message;
  const matcher = filter.regex
    ? createSafeRegex(filter.text, "g")
    : createLiteralRegex(filter.text, "g");
  if (!matcher) return message;
  const parts: ReactNode[] = [];
  let cursor = 0;
  let matchCount = 0;
  for (const match of message.matchAll(matcher)) {
    if (matchCount >= MAX_HIGHLIGHTS) break;
    const index = match.index ?? 0;
    if (index > cursor) parts.push(message.slice(cursor, index));
    parts.push(<mark key={`${index}-${matchCount}`}>{match[0]}</mark>);
    cursor = index + match[0].length;
    matchCount += 1;
  }
  if (cursor === 0) return message;
  if (cursor < message.length) parts.push(message.slice(cursor));
  return parts;
}

function App() {
  const [sources, setSources] = useState<SourceSpec[]>(() => isTauri() ? [] : [{ kind: "localFile", path: "fixture.log" }]);
  const [cursors, setCursors] = useState<Array<FileCursor | null>>(() => isTauri() ? [] : [null]);
  const [records, setRecords] = useState<LogRecord[]>(() => isTauri() ? [] : browserSnapshot().records);
  const [snapshot, setSnapshot] = useState<SourcesSnapshot | null>(() => isTauri() ? null : browserSnapshot());
  const [filter, setFilter] = useState<FilterSpec>({ text: "", regex: false });
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [bookmarks, setBookmarks] = useState<Set<string>>(new Set());
  const [savedViews, setSavedViews] = useState<SavedView[]>([]);
  const [viewName, setViewName] = useState("");
  const [selectedViewName, setSelectedViewName] = useState("");
  const [kind, setKind] = useState<SourceKind>("localFile");
  const [path, setPath] = useState("");
  const [pattern, setPattern] = useState("*.log");
  const [distro, setDistro] = useState("Ubuntu");
  const [unit, setUnit] = useState("");
  const [sourceId, setSourceId] = useState("");
  const [engine, setEngine] = useState<ContainerEngine>("docker");
  const [containerId, setContainerId] = useState("");
  const [follow, setFollow] = useState(false);
  const [paused, setPaused] = useState(false);
  const [wrapLines, setWrapLines] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [contextRecord, setContextRecord] = useState<LogRecord | null>(null);
  const generation = useRef(0);
  const operation = useRef<string | null>(null);
  const mounted = useRef(true);
  const refreshInFlight = useRef(false);
  const pausedRef = useRef(paused);
  const compositionRef = useRef(false);
  const contextRecordRef = useRef<LogRecord | null>(null);
  pausedRef.current = paused;

  const prepareLogContext = useCallback((_reason: "pointer" | "keyboard", target: HTMLElement) => {
    const element = target as HTMLElement | null;
    const key = element?.dataset?.logKey
      ?? element?.closest?.<HTMLElement>("[data-log-key]")?.dataset.logKey;
    const record = key
      ? records.find((candidate) => recordKey(candidate) === key) ?? null
      : contextRecordRef.current;
    contextRecordRef.current = record;
    setContextRecord(record);
  }, [records]);
  const logContextMenu = useContextMenu({ onBeforeOpen: prepareLogContext });
  const closeLogContextMenu = useCallback(() => {
    contextRecordRef.current = null;
    setContextRecord(null);
    logContextMenu.close();
  }, [logContextMenu.close]);

  const refresh = useCallback(async (
    nextSources = sources,
    nextCursors = cursors,
  ) => {
    if (nextSources.length === 0 || refreshInFlight.current) return;
    refreshInFlight.current = true;
    const currentGeneration = generation.current + 1;
    generation.current = currentGeneration;
    const operationId = makeOperationId();
    operation.current = operationId;
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const sequenceStarts = nextSources.map((_, index) => {
        if (!nextCursors[index]) return 0;
        const previousSourceId = snapshot?.sources[index]?.sourceId;
        return records
          .filter((record) => record.sourceId === previousSourceId)
          .reduce((maximum, record) => Math.max(maximum, record.sequence + 1), 0);
      });
      const next = await readSources(nextSources, nextCursors, sequenceStarts, currentGeneration, operationId);
      if (!mounted.current || generation.current !== currentGeneration || pausedRef.current) return;
      setSnapshot(next);
      setRecords(nextSources === sources
        ? (current) => mergeClientRecords(current, next.records, next.sources, next.cursors, next.statuses)
        : next.records);
      setCursors(next.cursors);
    } catch {
      if (mounted.current && generation.current === currentGeneration) {
        setError("The log source could not be read. Check the source and try again.");
      }
    } finally {
      refreshInFlight.current = false;
      if (mounted.current && generation.current === currentGeneration) {
        setBusy(false);
        operation.current = null;
      }
    }
  }, [cursors, paused, records, snapshot, sources]);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      generation.current += 1;
      const active = operation.current;
      operation.current = null;
      if (active) void cancelRead(active);
    };
  }, []);

  useEffect(() => {
    if (!follow || paused || sources.length === 0) return undefined;
    const timer = window.setInterval(() => void refresh(), 1_500);
    return () => window.clearInterval(timer);
  }, [follow, paused, refresh, sources.length]);

  const visibleRecords = useMemo(
    () => filterRecords(records, filter).slice(-MAX_RENDERED_ROWS),
    [filter, records],
  );

  const buildSource = (): SourceSpec | null => {
    switch (kind) {
      case "localFile": return path ? { kind, path } : null;
      case "directory": return path && pattern ? { kind, path, pattern } : null;
      case "wslFile": return distro && path ? { kind, distro, path } : null;
      case "wslJournal": return distro ? (unit ? { kind, distro, unit } : { kind, distro }) : null;
      case "run": return sourceId ? { kind, sourceId } : null;
      case "container": return containerId ? { kind, engine, containerId } : null;
    }
  };

  const addSource = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const nativeEvent = event.nativeEvent as SubmitEvent & { isComposing?: boolean };
    if (compositionRef.current || nativeEvent.isComposing) return;
    const source = buildSource();
    if (!source) {
      setError("Enter a valid source value.");
      return;
    }
    if (sources.some((candidate) => JSON.stringify(candidate) === JSON.stringify(source))) {
      setError("This source is already selected.");
      setNotice(null);
      return;
    }
    if (sources.length >= MAX_SOURCES) {
      setError(`A maximum of ${MAX_SOURCES} sources can be loaded at once.`);
      setNotice(null);
      return;
    }
    const nextSources = [...sources, source];
    const nextCursors = nextSources.map(() => null);
    setSources(nextSources);
    setCursors(nextCursors);
    setError(null);
    setNotice(null);
    void refresh(nextSources, nextCursors);
  };

  const removeSource = (index: number) => {
    const nextSources = sources.filter((_, sourceIndex) => sourceIndex !== index);
    const nextCursors = cursors.filter((_, sourceIndex) => sourceIndex !== index);
    setSources(nextSources);
    setCursors(nextCursors);
    setError(null);
    setNotice(null);
    if (nextSources.length) void refresh(nextSources, nextCursors);
    else {
      generation.current += 1;
      setRecords([]);
      setSnapshot(null);
    }
  };

  const saveView = () => {
    const name = viewName.trim();
    if (!name || name.length > 128 || !sources.length) {
      setError("A view name and at least one source are required.");
      setNotice(null);
      return;
    }
    const replacing = savedViews.some((view) => view.name === name);
    const evictsOldest = !replacing && savedViews.length >= MAX_SAVED_VIEWS;
    setSavedViews((current) => [...current.filter((view) => view.name !== name), {
      name,
      sources: sources.map((source) => ({ ...source })),
      filter: { ...filter },
    }].slice(-MAX_SAVED_VIEWS));
    setSelectedViewName(name);
    setViewName("");
    setError(null);
    setNotice(evictsOldest
      ? `Saved view limit reached; the oldest view was removed.`
      : replacing
        ? `Saved view “${name}” updated.`
        : `Saved view “${name}” saved.`);
  };

  const loadView = (name: string) => {
    setSelectedViewName(name);
    const view = savedViews.find((candidate) => candidate.name === name);
    if (!view) return;
    const nextSources = view.sources.map((source) => ({ ...source })) as SourceSpec[];
    const nextCursors = nextSources.map(() => null);
    setSources(nextSources);
    setCursors(nextCursors);
    setFilter({ ...view.filter });
    setError(null);
    void refresh(nextSources, nextCursors);
    setNotice(`Loaded saved view “${name}”.`);
  };

  const deleteSavedView = () => {
    if (!selectedViewName) return;
    const deletedName = selectedViewName;
    setSavedViews((current) => current.filter((view) => view.name !== deletedName));
    setSelectedViewName("");
    setError(null);
    setNotice(`Saved view “${deletedName}” removed.`);
  };

  const logContextItems = useMemo<readonly ContextMenuEntry[]>(() => {
    if (!contextRecord) return [];
    const key = recordKey(contextRecord);
    return [
      { type: "item", id: "copy-line", label: "Copy log line", shortcut: "Ctrl+C" },
      {
        type: "item",
        id: "toggle-bookmark",
        label: bookmarks.has(key) ? "Remove bookmark" : "Add bookmark",
      },
      {
        type: "item",
        id: "toggle-selection",
        label: selected.has(key) ? "Deselect line" : "Select line",
      },
    ];
  }, [bookmarks, contextRecord, selected]);

  const onLogContextSelect = useCallback((id: string) => {
    const record = contextRecordRef.current;
    if (!record) return;
    const key = recordKey(record);
    if (id === "copy-line") {
      void exportRecords([record])
        .then(async (exported) => {
          if (!navigator.clipboard?.writeText) throw new Error("clipboard unavailable");
          await navigator.clipboard.writeText(exported.text.trimEnd());
          if (mounted.current) {
            setError(null);
            setNotice("Log line copied.");
          }
        })
        .catch(() => {
          if (mounted.current) {
            setError("Clipboard copy failed.");
            setNotice(null);
          }
        });
      return;
    }
    if (id === "toggle-bookmark") {
      setBookmarks((current) => {
        const next = new Set(current);
        if (next.has(key)) next.delete(key); else next.add(key);
        return next;
      });
      setNotice("Bookmark updated.");
    } else if (id === "toggle-selection") {
      setSelected((current) => {
        const next = new Set(current);
        if (next.has(key)) next.delete(key); else next.add(key);
        return next;
      });
      setNotice("Selection updated.");
    }
    setError(null);
  }, []);

  const toggleSelection = (record: LogRecord) => {
    const key = recordKey(record);
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key); else next.add(key);
      return next;
    });
  };

  const toggleBookmark = (record: LogRecord) => {
    const key = recordKey(record);
    setBookmarks((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key); else next.add(key);
      return next;
    });
  };

  const exportVisible = async (copy: boolean) => {
    const targets = visibleRecords.filter((record) => selected.has(recordKey(record)));
    const exportTargets = targets.length ? targets : visibleRecords;
    if (!exportTargets.length) return;
    try {
      const exported = await exportRecords(exportTargets);
      if (copy) {
        await navigator.clipboard.writeText(exported.text);
        return;
      }
      const url = URL.createObjectURL(new Blob([exported.text], { type: "text/plain;charset=utf-8" }));
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = "log-lens-export.txt";
      anchor.style.display = "none";
      document.body.append(anchor);
      anchor.click();
      anchor.remove();
      window.setTimeout(() => URL.revokeObjectURL(url), 0);
      if (exported.truncated) setError("Export reached the safety limit and was truncated.");
    } catch {
      setError(copy ? "Clipboard copy failed." : "Export failed.");
    }
  };

  return (
    <main className="app-shell">
      <header className="topbar">
        <div>
          <p className="eyebrow">DEVBOX · OFFLINE</p>
          <h1>Log Lens</h1>
          <p className="subtitle">Bounded local log tail, merge, and inspection</p>
        </div>
        <div className="top-actions">
          <label className="toggle"><input type="checkbox" checked={follow} onChange={(event) => setFollow(event.target.checked)} /> Follow</label>
          <label className="toggle"><input type="checkbox" checked={paused} onChange={(event) => setPaused(event.target.checked)} /> Pause rendering</label>
          <label className="toggle"><input type="checkbox" checked={wrapLines} onChange={(event) => setWrapLines(event.target.checked)} /> Wrap lines</label>
          <button className="button" type="button" onClick={() => void refresh()} disabled={busy || !sources.length}>Refresh</button>
          <button className="button danger" type="button" onClick={() => { const active = operation.current; if (active) void cancelRead(active); }} disabled={!busy}>Cancel</button>
        </div>
      </header>

      <section className="source-panel" aria-labelledby="sources-heading">
        <div className="section-heading"><h2 id="sources-heading">Sources</h2><span className="muted">{sources.length}/{MAX_SOURCES} selected · Read-only · no network ingest · no archive</span></div>
        <form className="source-form" onSubmit={addSource} onCompositionStart={() => { compositionRef.current = true; }} onCompositionEnd={() => { compositionRef.current = false; }}>
          <label>Type<select value={kind} onChange={(event) => setKind(event.target.value as SourceKind)}>{(["localFile", "directory", "wslFile", "wslJournal", "run", "container"] as SourceKind[]).map((value) => <option key={value} value={value}>{labelForKind(value)}</option>)}</select></label>
          {(kind === "localFile" || kind === "directory" || kind === "wslFile") && <label>Path<input value={path} onChange={(event) => setPath(truncateUtf8(event.target.value, 4 * 1024))} placeholder={kind === "wslFile" ? "/var/log/app.log" : "C:\\logs\\app.log"} /></label>}
          {kind === "directory" && <label>Pattern<input value={pattern} onChange={(event) => setPattern(truncateUtf8(event.target.value, 128))} placeholder="*.log" /></label>}
          {(kind === "wslFile" || kind === "wslJournal") && <label>Distro<input value={distro} onChange={(event) => setDistro(truncateUtf8(event.target.value, 128))} placeholder="Ubuntu" /></label>}
          {kind === "wslJournal" && <label>Unit (optional)<input value={unit} onChange={(event) => setUnit(truncateUtf8(event.target.value, 128))} placeholder="sshd.service" /></label>}
          {kind === "run" && <label>Opaque source ID<input value={sourceId} onChange={(event) => setSourceId(truncateUtf8(event.target.value, 192))} placeholder="run-manager:run-1:stdout" /></label>}
          {kind === "container" && <><label>Engine<select value={engine} onChange={(event) => setEngine(event.target.value as ContainerEngine)}><option value="docker">Docker</option><option value="podman">Podman</option></select></label><label>Container ID/name<input value={containerId} onChange={(event) => setContainerId(truncateUtf8(event.target.value, 128))} /></label></>}
          <button className="button primary" type="submit" disabled={busy}>Add source</button>
        </form>
        <div className="source-list" aria-live="polite">{sources.length ? sources.map((source, index) => {
          const status = snapshot?.statuses[index];
          return <div className="source-chip" key={`${source.kind}-${index}`}><span>{labelForKind(source.kind)}</span><span className={`source-status source-status-${status ?? "pending"}`} aria-label={`Status: ${status ?? "pending"}`}>{status ?? "pending"}</span><button type="button" disabled={busy} aria-label={`Remove ${labelForKind(source.kind)}`} onClick={() => removeSource(index)}>×</button></div>;
        }) : <span className="muted">No source selected. Browser mode shows a bounded fixture.</span>}</div>
      </section>

      <section className="filter-panel" aria-labelledby="filter-heading">
        <div className="section-heading"><h2 id="filter-heading">Filter</h2><span className="muted">{visibleRecords.length} shown · {records.length} retained</span></div>
        <div className="filter-row"><label className="filter-grow">Text<input value={filter.text} onChange={(event) => setFilter((current) => ({ ...current, text: truncateUtf8(event.target.value, 512) }))} placeholder="message or field value" /></label><label className="toggle"><input type="checkbox" checked={filter.regex} onChange={(event) => setFilter((current) => ({ ...current, regex: event.target.checked }))} /> Regex</label><label>Level<select value={filter.level ?? ""} onChange={(event) => setFilter((current) => ({ ...current, level: (event.target.value || undefined) as LogLevel | undefined }))}><option value="">All levels</option>{(["trace", "debug", "info", "warn", "error", "fatal"] as LogLevel[]).map((value) => <option key={value} value={value}>{value}</option>)}</select></label><label>Source<select value={filter.sourceId ?? ""} onChange={(event) => setFilter((current) => ({ ...current, sourceId: event.target.value || undefined }))}><option value="">All sources</option>{(snapshot?.sources ?? []).map((source) => <option key={source.sourceId} value={source.sourceId}>{source.displayName}</option>)}</select></label><button className="button" type="button" onClick={() => void exportVisible(false)} disabled={!visibleRecords.length}>Export</button><button className="button" type="button" onClick={() => void exportVisible(true)} disabled={!visibleRecords.length}>Copy</button></div>
        <div className="filter-row"><label>Field<input value={filter.field ?? ""} onChange={(event) => setFilter((current) => ({ ...current, field: event.target.value ? truncateUtf8(event.target.value, 4 * 1024) : undefined }))} placeholder="field name" /></label><label>Field value<input value={filter.fieldValue ?? ""} onChange={(event) => setFilter((current) => ({ ...current, fieldValue: event.target.value ? truncateUtf8(event.target.value, 4 * 1024) : undefined }))} placeholder="value" /></label><label>Start epoch ms<input type="number" value={filter.startAt ?? ""} onChange={(event) => setFilter((current) => ({ ...current, startAt: event.target.value ? Number(event.target.value) : undefined }))} /></label><label>End epoch ms<input type="number" value={filter.endAt ?? ""} onChange={(event) => setFilter((current) => ({ ...current, endAt: event.target.value ? Number(event.target.value) : undefined }))} /></label><label>Save view<input value={viewName} onChange={(event) => setViewName(truncateUtf8(event.target.value, 128))} placeholder="view name" /></label><button className="button" type="button" onClick={saveView} disabled={!sources.length}>Save</button><select aria-label="Load saved view" value={selectedViewName} onChange={(event) => loadView(event.target.value)}><option value="">Load view…</option>{savedViews.map((view) => <option key={view.name} value={view.name}>{view.name}</option>)}</select><button className="button" type="button" onClick={deleteSavedView} disabled={!selectedViewName}>Remove view</button></div>
      </section>

      {error && <p className="error" role="alert">{error}</p>}
      {notice && <p className="notice" role="status" aria-live="polite">{notice}</p>}
      <section className="log-panel" aria-labelledby="log-heading">
        <div className="section-heading"><h2 id="log-heading">Merged output</h2><span className="muted" aria-live="polite">{busy ? "Reading…" : snapshot?.truncated ? "Safety limit reached" : "Ready"}</span></div>
        <div className="log-table" role="table" aria-label="Merged log records" aria-busy={busy}>
          <div className="log-row log-head" role="row"><span role="columnheader" aria-label="Selection" /><span role="columnheader">Time</span><span role="columnheader">Level</span><span role="columnheader">Source</span><span role="columnheader">Message</span><span role="columnheader">Bookmark</span></div>
          {visibleRecords.map((record) => { const key = recordKey(record); return <div className="log-row" role="row" tabIndex={0} key={key} data-log-key={key} aria-selected={selected.has(key)} aria-label={`Log line ${record.sequence}: ${record.message}`} {...logContextMenu.triggerProps} onContextMenu={(event) => { contextRecordRef.current = record; setContextRecord(record); logContextMenu.triggerProps.onContextMenu?.(event); }} onKeyDown={(event) => { contextRecordRef.current = record; setContextRecord(record); logContextMenu.triggerProps.onKeyDown?.(event); }}><span role="cell" className="row-select"><input type="checkbox" aria-label={`Select log line ${record.sequence}`} checked={selected.has(key)} onChange={() => toggleSelection(record)} /></span><time role="cell" dateTime={toIsoTimestamp(record.timestampMillis)}>{formatTimestamp(record.timestampMillis)}</time><span role="cell" className={`level level-${record.level ?? "unknown"}`}>{record.level ?? "—"}</span><code role="cell" title={record.sourceId}>{record.sourceId.slice(-12)}</code><span role="cell" className={`message ${wrapLines ? "" : "nowrap"}`}>{highlightMessage(record.message, filter)}</span><span role="cell" className="bookmark-cell"><button className={`bookmark ${bookmarks.has(key) ? "active" : ""}`} type="button" aria-label={`${bookmarks.has(key) ? "Remove" : "Add"} bookmark`} aria-pressed={bookmarks.has(key)} onClick={() => toggleBookmark(record)}>★</button></span></div>; })}
          {!visibleRecords.length && <p className="empty">No records match the current filter.</p>}
        </div>
      </section>
      <ContextMenu
        open={logContextMenu.open && contextRecord !== null}
        anchor={logContextMenu.anchor}
        restoreFocusTo={logContextMenu.restoreFocusTo}
        items={logContextItems}
        onSelect={onLogContextSelect}
        onClose={closeLogContextMenu}
        ariaLabel="Log line actions"
      />
      <footer className="statusbar" aria-live="polite"><span>{snapshot?.sources.length ?? 0} source(s)</span><span>{snapshot?.droppedRecords ?? 0} evicted by ring buffer</span><span>Bookmarks: {bookmarks.size}</span><span>Saved views: {savedViews.length}/{MAX_SAVED_VIEWS}</span></footer>
    </main>
  );
}

export default App;
