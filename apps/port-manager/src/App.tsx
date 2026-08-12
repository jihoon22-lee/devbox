import { useCallback, useEffect, useMemo, useState } from "react";
import { killProcess, listPorts, openBrowser } from "./api";
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

export default function App() {
  const [ports, setPorts] = useState<PortRow[]>([]);
  const [query, setQuery] = useState("");
  const [protoFilter, setProtoFilter] = useState<ProtoFilter>("all");
  const [stateFilter, setStateFilter] = useState<StateFilter>("all");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busyPid, setBusyPid] = useState<number | null>(null);

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

  const onKill = async (row: PortRow) => {
    if (row.pid == null) return;
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

  const onOpen = (row: PortRow) => {
    if (!row.port) return;
    void openBrowser(`http://localhost:${row.port}`);
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
            {visible.map((row) => (
              <tr key={`${row.proto}:${row.local_addr}:${row.pid ?? 0}`}>
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
                    <button className="btn" onClick={() => onOpen(row)}>
                      Open
                    </button>
                  )}
                </td>
              </tr>
            ))}
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
    </div>
  );
}
