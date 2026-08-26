import {
  ContextMenu,
  useContextMenu,
  type ContextMenuEntry,
} from "@devbox/context-menu";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  getProcessInfo,
  handoffContainerStop,
  killListener,
  listPorts,
  openBrowser,
  revealProcess,
} from "./api";
import type {
  ListenerActionResult,
  ListenerKillRequest,
  PortRow,
  ProtoFilter,
  StateFilter,
} from "./types";
import "./App.css";

const PROTO_FILTERS: { value: ProtoFilter; label: string }[] = [
  { value: "all", label: "All" },
  { value: "tcp", label: "TCP" },
  { value: "udp", label: "UDP" },
];

const STATE_FILTERS: { value: StateFilter; label: string }[] = [
  { value: "all", label: "All states" },
  { value: "listening", label: "LISTENING" },
  { value: "established", label: "ESTABLISHED" },
];

export function matches(row: PortRow, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return (
    row.proto.toLowerCase().includes(q) ||
    row.state.toLowerCase().includes(q) ||
    String(row.port).includes(q) ||
    (row.pid != null && String(row.pid).includes(q)) ||
    row.local_addr.toLowerCase().includes(q) ||
    (row.process_name?.toLowerCase().includes(q) ?? false) ||
    (row.command_line?.toLowerCase().includes(q) ?? false) ||
    (row.executable_path?.toLowerCase().includes(q) ?? false) ||
    (row.wsl_distro?.toLowerCase().includes(q) ?? false) ||
    (row.container_id?.toLowerCase().includes(q) ?? false) ||
    (row.container_name?.toLowerCase().includes(q) ?? false)
  );
}

export function portRowKey(row: PortRow): string {
  const identity = row.identity;
  if (identity?.kind === "windows") {
    return (
      row.proto +
      ":" +
      row.local_addr +
      ":" +
      identity.pid +
      ":" +
      identity.start_time
    );
  }
  if (identity?.kind === "wsl") {
    return (
      row.proto +
      ":" +
      row.local_addr +
      ":" +
      identity.distro +
      ":" +
      identity.pid +
      ":" +
      identity.start_tick
    );
  }
  if (identity?.kind === "container") {
    return (
      row.proto +
      ":" +
      row.local_addr +
      ":" +
      identity.distro +
      ":" +
      identity.container_id
    );
  }
  return row.proto + ":" + row.local_addr + ":" + (row.pid ?? 0);
}

export function localhostUrl(row: PortRow): string | null {
  return row.port > 0 ? "http://localhost:" + row.port : null;
}

export function listenerKillRequest(row: PortRow): ListenerKillRequest | null {
  if (!row.identity) return null;
  return {
    endpoint: {
      proto: row.proto,
      local_addr: row.local_addr,
      port: row.port,
      state: row.state,
    },
    identity: row.identity,
  };
}

export function sourceLabel(row: PortRow): string {
  switch (row.source ?? "windows") {
    case "wsl":
      return "WSL";
    case "container":
      return "Container";
    default:
      return "Windows";
  }
}

export function safeActionError(_error: unknown): string {
  return "Action failed. Refresh the list and try again.";
}

export function shouldIgnoreComposingShortcut(isComposing: boolean, key: string): boolean {
  return isComposing && (key === "Enter" || key === " " || key === "F10");
}

export function isCurrentRequest(request: number, current: number): boolean {
  return request === current;
}

function isListening(row: PortRow): boolean {
  return row.state.toLowerCase() === "listening";
}

function isListener(row: PortRow): boolean {
  const state = row.state.toLowerCase();
  return state === "" || state === "listening" || state === "unconn" || state === "bound";
}

function matchesStateFilter(row: PortRow, filter: StateFilter): boolean {
  if (filter === "all") return true;
  if (filter === "listening") return isListener(row);
  return row.state.toLowerCase() === filter;
}

function displayValue(value: string | number | null | undefined): string {
  return value === null || value === undefined || value === "" ? "-" : String(value);
}

type ProcessPathState = {
  rowKey: string;
  path: string | null;
};

export default function App() {
  const [ports, setPorts] = useState<PortRow[]>([]);
  const [query, setQuery] = useState("");
  const [protoFilter, setProtoFilter] = useState<ProtoFilter>("all");
  const [stateFilter, setStateFilter] = useState<StateFilter>("all");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busyRowKey, setBusyRowKey] = useState<string | null>(null);
  const [selectedRowKey, setSelectedRowKey] = useState<string | null>(null);
  const [contextRow, setContextRow] = useState<PortRow | null>(null);
  const [processPath, setProcessPath] = useState<ProcessPathState | null>(null);
  const [handoff, setHandoff] = useState<string | null>(null);
  const [isComposing, setIsComposing] = useState(false);
  const processPathRequest = useRef(0);
  const refreshRequest = useRef(0);
  const busyActionRef = useRef<string | null>(null);
  const mounted = useRef(false);

  const refresh = useCallback(async () => {
    if (!mounted.current) return;
    const request = ++refreshRequest.current;
    setLoading(true);
    setError(null);
    try {
      const next = await listPorts();
      if (mounted.current && refreshRequest.current === request) {
        setPorts(next);
      }
    } catch (caught) {
      if (mounted.current && refreshRequest.current === request) {
        setError(safeActionError(caught));
      }
    } finally {
      if (mounted.current && refreshRequest.current === request) {
        setLoading(false);
      }
    }
  }, []);

  useEffect(() => {
    mounted.current = true;
    void refresh();
    return () => {
      mounted.current = false;
      refreshRequest.current += 1;
      processPathRequest.current += 1;
      busyActionRef.current = null;
    };
  }, [refresh]);

  const visible = useMemo(() => {
    return ports.filter(
      (row) =>
        matches(row, query) &&
        (protoFilter === "all" || row.proto.toLowerCase().startsWith(protoFilter)) &&
        matchesStateFilter(row, stateFilter),
    );
  }, [ports, query, protoFilter, stateFilter]);

  const counts = useMemo(() => {
    const listening = ports.filter((p) => isListener(p)).length;
    return { total: ports.length, listening };
  }, [ports]);

  const runAction = useCallback(async (action: () => Promise<void>) => {
    if (!mounted.current) return;
    setError(null);
    try {
      await action();
    } catch (caught) {
      if (mounted.current) setError(safeActionError(caught));
    }
  }, []);

  const onKill = async (row: PortRow) => {
    if (!mounted.current || busyActionRef.current !== null) return;
    const request = listenerKillRequest(row);
    if (!request || !isListener(row)) {
      setError("Identity unavailable. Refresh the list before trying again.");
      return;
    }
    const processLabel = row.process_name ? " (" + row.process_name + ")" : "";
    const actionLabel = row.source === "container" ? "WSL Desktop에서 중지" : "listener 종료";
    if (!window.confirm(row.local_addr + processLabel + " " + actionLabel + "할까요?")) return;

    const rowKey = portRowKey(row);
    busyActionRef.current = rowKey;
    setBusyRowKey(rowKey);
    setError(null);
    setHandoff(null);
    try {
      const result: ListenerActionResult =
        row.source === "container"
          ? { kind: "handoff", handoff: await handoffContainerStop(request) }
          : await killListener(request);
      if (result.kind === "handoff" && mounted.current) {
        setHandoff(
          "WSL Desktop에서 " + result.handoff.container_id + " 컨테이너를 중지하세요.",
        );
      } else {
        await refresh();
      }
    } catch (caught) {
      if (mounted.current) setError(safeActionError(caught));
    } finally {
      if (busyActionRef.current === rowKey) {
        busyActionRef.current = null;
        if (mounted.current) setBusyRowKey(null);
      }
    }
  };

  const onOpen = async (row: PortRow) => {
    const url = localhostUrl(row);
    if (!url || !isListening(row)) return;
    await runAction(() => openBrowser(url));
  };

  const prepareContextRow = useCallback(
    (target: HTMLElement) => {
      const rowKey = target.dataset.portRowKey;
      const row = ports.find((candidate) => portRowKey(candidate) === rowKey);
      if (!row || !rowKey) return;

      setSelectedRowKey(rowKey);
      setContextRow(row);
      setProcessPath(null);

      const request = ++processPathRequest.current;
      if (row.pid == null || row.source === "wsl" || row.source === "container") return;
      void getProcessInfo(row.pid)
        .then((info) => {
          if (mounted.current && processPathRequest.current === request) {
            setProcessPath({ rowKey, path: info.executable_path ?? info.exe });
          }
        })
        .catch(() => {
          if (mounted.current && processPathRequest.current === request) {
            setProcessPath({ rowKey, path: null });
          }
        });
    },
    [ports],
  );

  const contextMenu = useContextMenu({
    onBeforeOpen: (_reason, target) => prepareContextRow(target),
  });

  const contextPath =
    contextRow && processPath?.rowKey === portRowKey(contextRow) ? processPath.path : null;
  const contextMenuItems = useMemo<readonly ContextMenuEntry[]>(() => {
    if (!contextRow) return [];
    const url = localhostUrl(contextRow);
    const hasPid = contextRow.pid != null;
    const request = listenerKillRequest(contextRow);
    const isContainer = contextRow.source === "container";
    const isBusy = busyRowKey === portRowKey(contextRow);
    return [
      { type: "item", id: "copy-port", label: "Copy port", disabled: contextRow.port <= 0 },
      { type: "item", id: "copy-pid", label: "Copy PID", disabled: !hasPid },
      { type: "item", id: "copy-localhost-url", label: "Copy localhost URL", disabled: !url },
      {
        type: "item",
        id: "open-localhost",
        label: "Open localhost",
        disabled: !url || !isListening(contextRow),
      },
      { type: "separator", id: "process-separator" },
      {
        type: "item",
        id: "copy-process-path",
        label: "Copy process path",
        disabled: !contextPath,
      },
      {
        type: "item",
        id: "reveal-process",
        label: "Show in Explorer",
        disabled: !hasPid || !contextPath || contextRow.source !== "windows",
      },
      { type: "separator", id: "danger-separator" },
      {
        type: "item",
        id: isContainer ? "handoff-container-stop" : "kill-listener",
        label: isContainer
          ? isBusy
            ? "Preparing handoff…"
            : "Stop in WSL Desktop"
          : isBusy
            ? "Killing…"
            : "Kill listener",
        disabled: !request || !isListener(contextRow) || isBusy,
        danger: true,
      },
    ];
  }, [busyRowKey, contextPath, contextRow]);

  const onContextMenuSelect = (id: string) => {
    const row = contextRow;
    if (!row) return;
    const url = localhostUrl(row);

    switch (id) {
      case "copy-port":
        if (row.port > 0) void runAction(() => navigator.clipboard.writeText(String(row.port)));
        break;
      case "copy-pid":
        if (row.pid != null) void runAction(() => navigator.clipboard.writeText(String(row.pid)));
        break;
      case "copy-localhost-url":
        if (url) void runAction(() => navigator.clipboard.writeText(url));
        break;
      case "open-localhost":
        void onOpen(row);
        break;
      case "copy-process-path":
        if (contextPath) void runAction(() => navigator.clipboard.writeText(contextPath));
        break;
      case "reveal-process":
        if (row.pid != null && contextPath) {
          const pid = row.pid;
          void runAction(() => revealProcess(pid));
        }
        break;
      case "kill-listener":
      case "handoff-container-stop":
        void onKill(row);
        break;
    }
  };

  const selectedRow = selectedRowKey
    ? ports.find((row) => portRowKey(row) === selectedRowKey) ?? null
    : null;

  return (
    <div className="app">
      <header className="toolbar">
        <h1 className="title">Port Manager</h1>
        <input
          className="search"
          aria-label="Search listeners"
          placeholder="Search (port / proto / pid / process)..."
          value={query}
          onChange={(event) => setQuery(event.currentTarget.value)}
          onCompositionStart={() => setIsComposing(true)}
          onCompositionEnd={() => setIsComposing(false)}
          onKeyDown={(event) => {
            if (shouldIgnoreComposingShortcut(isComposing, event.key)) {
              event.stopPropagation();
            }
          }}
        />
        <div className="filters" aria-label="Listener filters">
          {PROTO_FILTERS.map((filter) => (
            <button
              key={filter.value}
              type="button"
              className={"chip " + (protoFilter === filter.value ? "active" : "")}
              aria-pressed={protoFilter === filter.value}
              onClick={() => setProtoFilter(filter.value)}
            >
              {filter.label}
            </button>
          ))}
          <span className="divider" />
          {STATE_FILTERS.map((filter) => (
            <button
              key={filter.value}
              type="button"
              className={"chip " + (stateFilter === filter.value ? "active" : "")}
              aria-pressed={stateFilter === filter.value}
              onClick={() => setStateFilter(filter.value)}
            >
              {filter.label}
            </button>
          ))}
        </div>
        <button
          type="button"
          className="btn refresh"
          onClick={() => void refresh()}
          disabled={loading}
        >
          {loading ? "Refreshing..." : "Refresh"}
        </button>
      </header>

      {error && (
        <div className="error" role="alert" aria-live="assertive">
          {error}
        </div>
      )}
      {handoff && (
        <div className="handoff" role="status" aria-live="polite">
          {handoff}
        </div>
      )}

      <div className="statusbar" aria-live="polite">
        <span>
          {visible.length} / {counts.total} rows
        </span>
        <span className="dot-green" /> {counts.listening} listeners
      </div>

      <div className="table-wrap">
        <table aria-label="Listener list">
          <thead>
            <tr>
              <th>SOURCE</th>
              <th>PROTO</th>
              <th>PORT</th>
              <th>LOCAL ADDRESS</th>
              <th>STATE</th>
              <th>PID</th>
              <th>PROCESS</th>
              <th>ACTION</th>
            </tr>
          </thead>
          <tbody>
            {visible.map((row) => {
              const rowKey = portRowKey(row);
              const selected = selectedRowKey === rowKey;
              const busy = busyRowKey === rowKey;
              return (
                <tr
                  key={rowKey}
                  data-port-row-key={rowKey}
                  tabIndex={0}
                  aria-selected={selected}
                  aria-label={sourceLabel(row) + " " + row.local_addr}
                  className={selected ? "selected" : undefined}
                  {...contextMenu.triggerProps}
                  onClick={() => setSelectedRowKey(rowKey)}
                  onKeyDown={(event) => {
                    contextMenu.triggerProps.onKeyDown?.(event);
                    if (event.defaultPrevented) return;
                    if (shouldIgnoreComposingShortcut(isComposing, event.key)) return;
                    if (event.key === "Enter" || event.key === " ") {
                      event.preventDefault();
                      setSelectedRowKey(rowKey);
                    }
                  }}
                >
                  <td>{sourceLabel(row)}</td>
                  <td>{row.proto}</td>
                  <td className="mono">{row.port || "-"}</td>
                  <td className="mono dim">{row.local_addr}</td>
                  <td>{row.state || "-"}</td>
                  <td className="mono">{row.pid ?? "-"}</td>
                  <td>{row.process_name ?? "-"}</td>
                  <td className="actions">
                    {row.identity && isListener(row) && (
                      <button
                        type="button"
                        className="btn danger"
                        aria-label={
                          row.source === "container" ? "Stop in WSL Desktop" : "Kill listener"
                        }
                        disabled={busy}
                        onClick={() => void onKill(row)}
                      >
                        {busy
                          ? row.source === "container"
                            ? "Preparing..."
                            : "Killing..."
                          : row.source === "container"
                            ? "Stop"
                            : "Kill"}
                      </button>
                    )}
                    {row.port > 0 && isListening(row) && (
                      <button type="button" className="btn" onClick={() => void onOpen(row)}>
                        Open
                      </button>
                    )}
                  </td>
                </tr>
              );
            })}
            {visible.length === 0 && (
              <tr>
                <td colSpan={8} className="empty">
                  No results
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      {selectedRow && (
        <aside className="details" aria-label="Listener details">
          <div className="details-heading">
            <h2>Listener details</h2>
            <span>{sourceLabel(selectedRow)}</span>
          </div>
          <dl>
            <div>
              <dt>Endpoint</dt>
              <dd className="mono">
                {selectedRow.proto} {selectedRow.local_addr}
              </dd>
            </div>
            <div>
              <dt>PID</dt>
              <dd className="mono">{displayValue(selectedRow.pid)}</dd>
            </div>
            <div>
              <dt>Process</dt>
              <dd>{displayValue(selectedRow.process_name)}</dd>
            </div>
            <div>
              <dt>Command line</dt>
              <dd className="mono details-value">
                {displayValue(selectedRow.command_line)}
              </dd>
            </div>
            <div>
              <dt>Executable path</dt>
              <dd className="mono details-value">
                {displayValue(selectedRow.executable_path)}
              </dd>
            </div>
            <div>
              <dt>Process start time</dt>
              <dd className="mono">
                {displayValue(selectedRow.process_start_time)}
              </dd>
            </div>
            {selectedRow.wsl_distro && (
              <div>
                <dt>WSL distro</dt>
                <dd>{selectedRow.wsl_distro}</dd>
              </div>
            )}
            {selectedRow.wsl_start_tick != null && (
              <div>
                <dt>WSL start tick</dt>
                <dd className="mono">{selectedRow.wsl_start_tick}</dd>
              </div>
            )}
            {selectedRow.container_id && (
              <div>
                <dt>Container</dt>
                <dd className="mono">
                  {displayValue(selectedRow.container_engine)} / {selectedRow.container_id}
                </dd>
              </div>
            )}
          </dl>
          {selectedRow.source === "container" && listenerKillRequest(selectedRow) && (
            <button
              type="button"
              className="btn danger"
              disabled={busyRowKey === portRowKey(selectedRow)}
              onClick={() => void onKill(selectedRow)}
            >
              Stop in WSL Desktop
            </button>
          )}
        </aside>
      )}

      <ContextMenu
        open={contextMenu.open}
        anchor={contextMenu.anchor}
        restoreFocusTo={contextMenu.restoreFocusTo}
        items={contextMenuItems}
        onSelect={onContextMenuSelect}
        onClose={contextMenu.close}
        ariaLabel="Port actions"
      />
    </div>
  );
}
