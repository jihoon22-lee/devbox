import {
  ContextMenu,
  useContextMenu,
  type ContextMenuEntry,
} from "@devbox/context-menu";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  getProcessInfo,
  killProcess,
  listPorts,
  openBrowser,
  revealProcess,
} from "./api";
import type { PortRow, ProtoFilter, StateFilter } from "./types";
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
    (row.process_name?.toLowerCase().includes(q) ?? false)
  );
}

export function portRowKey(row: PortRow): string {
  return `${row.proto}:${row.local_addr}:${row.pid ?? 0}`;
}

export function localhostUrl(row: PortRow): string | null {
  return row.port > 0 ? `http://localhost:${row.port}` : null;
}

function isListening(row: PortRow): boolean {
  return row.state.toLowerCase() === "listening";
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
  const [busyPid, setBusyPid] = useState<number | null>(null);
  const [selectedRowKey, setSelectedRowKey] = useState<string | null>(null);
  const [contextRow, setContextRow] = useState<PortRow | null>(null);
  const [processPath, setProcessPath] = useState<ProcessPathState | null>(null);
  const processPathRequest = useRef(0);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setPorts(await listPorts());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const visible = useMemo(() => {
    return ports.filter(
      (row) =>
        matches(row, query) &&
        (protoFilter === "all" || row.proto.toLowerCase().startsWith(protoFilter)) &&
        (stateFilter === "all" || row.state.toLowerCase() === stateFilter),
    );
  }, [ports, query, protoFilter, stateFilter]);

  const counts = useMemo(() => {
    const listening = ports.filter((p) => p.state.toLowerCase() === "listening").length;
    return { total: ports.length, listening };
  }, [ports]);

  const runAction = useCallback(async (action: () => Promise<void>) => {
    setError(null);
    try {
      await action();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  const onKill = async (row: PortRow) => {
    if (row.pid == null) return;
    const processLabel = row.process_name ? ` (${row.process_name})` : "";
    if (!window.confirm(`PID ${row.pid}${processLabel} 프로세스를 종료할까요?`)) return;
    setBusyPid(row.pid);
    setError(null);
    try {
      await killProcess(row.pid);
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusyPid(null);
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
      if (row.pid == null) return;
      void getProcessInfo(row.pid)
        .then((info) => {
          if (processPathRequest.current === request) {
            setProcessPath({ rowKey, path: info.exe });
          }
        })
        .catch(() => {
          if (processPathRequest.current === request) {
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
        disabled: !hasPid || !contextPath,
      },
      { type: "separator", id: "danger-separator" },
      {
        type: "item",
        id: "kill-process",
        label: busyPid === contextRow.pid ? "Killing…" : "Kill process",
        disabled: !hasPid || busyPid === contextRow.pid,
        danger: true,
      },
    ];
  }, [busyPid, contextPath, contextRow]);

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
      case "kill-process":
        void onKill(row);
        break;
    }
  };

  const stateLabel = (row: PortRow) => row.state || "-";

  return (
    <div className="app">
      <header className="toolbar">
        <h1 className="title">Port Manager</h1>
        <input
          className="search"
          placeholder="Search (port / proto / pid / process)..."
          value={query}
          onChange={(e) => setQuery(e.currentTarget.value)}
        />
        <div className="filters">
          {PROTO_FILTERS.map((f) => (
            <button
              key={f.value}
              className={`chip ${protoFilter === f.value ? "active" : ""}`}
              onClick={() => setProtoFilter(f.value)}
            >
              {f.label}
            </button>
          ))}
          <span className="divider" />
          {STATE_FILTERS.map((f) => (
            <button
              key={f.value}
              className={`chip ${stateFilter === f.value ? "active" : ""}`}
              onClick={() => setStateFilter(f.value)}
            >
              {f.label}
            </button>
          ))}
        </div>
        <button className="btn refresh" onClick={() => void refresh()} disabled={loading}>
          {loading ? "Refreshing..." : "Refresh"}
        </button>
      </header>

      {error && <div className="error">{error}</div>}

      <div className="statusbar">
        <span>
          {visible.length} / {counts.total} rows
        </span>
        <span className="dot-green" /> {counts.listening} listening
      </div>

      <div className="table-wrap">
        <table>
          <thead>
            <tr>
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
              return (
                <tr
                  key={rowKey}
                  data-port-row-key={rowKey}
                  tabIndex={0}
                  aria-selected={selected}
                  className={selected ? "selected" : undefined}
                  onClick={() => setSelectedRowKey(rowKey)}
                  {...contextMenu.triggerProps}
                >
                  <td>{row.proto}</td>
                  <td className="mono">{row.port || "-"}</td>
                  <td className="mono dim">{row.local_addr}</td>
                  <td>{stateLabel(row)}</td>
                  <td className="mono">{row.pid ?? "-"}</td>
                  <td>{row.process_name ?? "-"}</td>
                  <td className="actions">
                    {row.pid != null && (
                      <button
                        className="btn danger"
                        disabled={busyPid === row.pid}
                        onClick={() => void onKill(row)}
                      >
                        {busyPid === row.pid ? "Killing..." : "Kill"}
                      </button>
                    )}
                    {row.port > 0 && row.state.toLowerCase() === "listening" && (
                      <button className="btn" onClick={() => void onOpen(row)}>
                        Open
                      </button>
                    )}
                  </td>
                </tr>
              );
            })}
            {visible.length === 0 && (
              <tr>
                <td colSpan={7} className="empty">
                  No results
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
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
