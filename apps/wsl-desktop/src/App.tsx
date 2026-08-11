import { useCallback, useEffect, useRef, useState } from "react";
import { closeSession, listDistros, onTerminalClosed, onTerminalOutput, startSession } from "./api";
import TermPane from "./components/TermPane";
import "./App.css";

type Layout = "grid" | "cols" | "rows";

interface Pane {
  id: string;
  distro: string;
}

export default function App() {
  const [distros, setDistros] = useState<string[]>([]);
  const [selected, setSelected] = useState("");
  const [cwd, setCwd] = useState("");
  const [panes, setPanes] = useState<Pane[]>([]);
  const [layout, setLayout] = useState<Layout>("grid");
  const [broadcastOn, setBroadcastOn] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const writes = useRef(new Map<string, (data: string) => void>());

  const registerWrite = useCallback((id: string, fn: (data: string) => void) => {
    writes.current.set(id, fn);
  }, []);
  const unregisterWrite = useCallback((id: string) => {
    writes.current.delete(id);
  }, []);

  useEffect(() => {
    void listDistros().then((ds) => {
      setDistros(ds);
      setSelected((prev) => prev || ds[0] || "Ubuntu");
    });
  }, []);

  useEffect(() => {
    const unsubs: (() => void)[] = [];
    void onTerminalOutput(({ session_id, data }) => {
      writes.current.get(session_id)?.(data);
    }).then((u) => unsubs.push(u));
    void onTerminalClosed(({ session_id }) => {
      setPanes((prev) => prev.filter((p) => p.id !== session_id));
      writes.current.delete(session_id);
    }).then((u) => unsubs.push(u));
    return () => unsubs.forEach((u) => u());
  }, []);

  const openTerminal = async () => {
    setError(null);
    try {
      const id = await startSession(selected, cwd || undefined);
      setPanes((prev) => [...prev, { id, distro: selected }]);
      setCwd("");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const close = (id: string) => {
    setPanes((prev) => prev.filter((p) => p.id !== id));
    void closeSession(id);
  };

  const gridCols = panes.length === 0 ? 1 : Math.ceil(Math.sqrt(panes.length));
  const gridStyle =
    layout === "cols"
      ? { gridTemplateColumns: `repeat(${Math.max(1, panes.length)}, 1fr)` }
      : layout === "rows"
        ? { gridTemplateRows: `repeat(${Math.max(1, panes.length)}, 1fr)` }
        : { gridTemplateColumns: `repeat(${gridCols}, 1fr)` };

  return (
    <div className="app">
      <header className="toolbar">
        <h1 className="title">WSL Desktop</h1>
        <select value={selected} onChange={(e) => setSelected(e.currentTarget.value)}>
          {distros.map((d) => (
            <option key={d} value={d}>
              {d}
            </option>
          ))}
        </select>
        <input className="cwd" placeholder="Open path (optional, e.g. /mnt/c/projects)" value={cwd} onChange={(e) => setCwd(e.currentTarget.value)} onKeyDown={(e) => e.key === "Enter" && void openTerminal()} />
        <button className="btn" onClick={() => void openTerminal()}>
          + Terminal
        </button>
        <span className="spacer" />
        <label className="toggle">
          <input type="checkbox" checked={broadcastOn} onChange={(e) => setBroadcastOn(e.currentTarget.checked)} />
          broadcast
        </label>
        {(["grid", "cols", "rows"] as const).map((l) => (
          <button key={l} className={`btn ${layout === l ? "active" : ""}`} onClick={() => setLayout(l)}>
            {l}
          </button>
        ))}
      </header>

      {error && <div className="error">{error}</div>}

      <div className="panes" style={gridStyle}>
        {panes.map((p) => (
          <TermPane
            key={p.id}
            sessionId={p.id}
            title={p.distro}
            broadcastOn={broadcastOn}
            registerWrite={registerWrite}
            unregisterWrite={unregisterWrite}
            onClose={() => close(p.id)}
          />
        ))}
        {panes.length === 0 && (
          <div className="empty">
            No terminals. Select a distro and click "+ Terminal".
            <div className="dim">(터미널은 Windows에서 실행해야 동작합니다)</div>
          </div>
        )}
      </div>
    </div>
  );
}
