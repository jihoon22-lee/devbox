import { useCallback, useEffect, useRef, useState } from "react";
import type { Description } from "@devbox/product-shell/api";
import { DistroPanel, runDockerControl, type DashboardSnapshot, type DashboardFreshness } from "@devbox/workspace-features/terminal-dashboard";
import { componentCall } from "./native";
import "./RuntimeWsl.css";

export default function RuntimeWsl({ description, active }: { description: Description; active: boolean }) {
  const [snapshot, setSnapshot] = useState<DashboardSnapshot | null>(null);
  const [freshness, setFreshness] = useState<DashboardFreshness>("loading");
  const [selected, setSelected] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [issue, setIssue] = useState("");
  const [fileDistro, setFileDistro] = useState<string | null>(null);
  const [path, setPath] = useState("");
  const latest = useRef({ description, active }); latest.current = { description, active };
  const inFlight = useRef(false);
  const actionPending = useRef(false);
  const lastGood = useRef<DashboardSnapshot | null>(null);
  const mounted = useRef(true);
  const call = useCallback(<T,>(method: string, args: Record<string, unknown> = {}) => componentCall<T>(latest.current.description, "workspace.terminal", method, args, "runtime"), []);
  const refresh = useCallback(async () => {
    if (inFlight.current || !latest.current.active) return;
    inFlight.current = true;
    setFreshness(lastGood.current ? "refreshing" : "loading");
    try {
      const value = await call<DashboardSnapshot>("dashboard_snapshot");
      if (!mounted.current) return;
      if (!lastGood.current || value.revision >= lastGood.current.revision) {
        lastGood.current = value; setSnapshot(value);
        setSelected(current => value.distros.some(distro => distro.name === current) ? current : (value.distros.find(distro => distro.default) ?? value.distros[0])?.name ?? "");
      }
      setFreshness("fresh"); setIssue("");
    } catch { if (mounted.current) { setFreshness("error"); setIssue("WSL 상태를 읽지 못했습니다. 마지막으로 확인한 정보를 표시합니다."); } }
    finally { inFlight.current = false; }
  }, [call]);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  useEffect(() => {
    if (!active) return;
    void refresh();
    const timer = setInterval(() => {
      const value = lastGood.current;
      if (value && Date.now() > value.capturedAtMs + value.staleAfterMs) setFreshness(current => current === "error" ? current : "stale");
      if (!actionPending.current) void refresh();
    }, 10_000);
    return () => clearInterval(timer);
  }, [active, refresh]);
  const perform = async (key: string, operation: () => Promise<unknown>) => {
    if (actionPending.current) return;
    actionPending.current = true; setBusy(key); setIssue("");
    try { await operation(); await refresh(); }
    catch (error) { if (mounted.current) setIssue(error instanceof Error ? error.message : "WSL 작업을 완료하지 못했습니다."); }
    finally { actionPending.current = false; if (mounted.current) setBusy(null); }
  };
  const current = snapshot?.distros.find(distro => distro.name === selected);
  const actionable = Boolean(snapshot && Date.now() <= snapshot.capturedAtMs + snapshot.staleAfterMs && freshness !== "error");
  return <section className="workspace-wsl" aria-label="WSL 및 컨테이너">
    <h2>WSL 및 컨테이너</h2>
    {issue && <p role="status">{issue}</p>}
    <DistroPanel distros={snapshot?.distros ?? []} dashboardDistros={snapshot?.distros ?? []} selectedDistro={selected} onSelectDistro={setSelected}
      containers={current?.containers ?? []} dockerMissing={current?.dockerAvailability === "missing"} busy={busy}
      snapshotState={freshness} snapshotActionable={actionable} onRefresh={() => void refresh()}
      onAction={(id, action) => {
        if (!actionable || current?.dockerAvailability !== "available" || !current.containers.some(container => container.id === id)) return;
        void perform(`${id}:${action}`, () => runDockerControl(call, selected, id, action));
      }}
      onOpenTerminal={distro => void perform(`terminal:${distro}`, () => call("open_distro_terminal", { operationId: crypto.randomUUID(), distro }))}
      onOpenJournalInLogLens={distro => void perform(`journal:${distro}`, () => call("open_wsl_journal_in_log_lens", { distro, unit: null }))}
      onOpenFileInLogLens={distro => { setFileDistro(distro); setPath(""); }}
    />
    {fileDistro && <form onSubmit={event => {
      event.preventDefault();
      if (!path.startsWith("/") || path.split("/").includes("..") || /[\x00-\x1f\x7f]/.test(path)) { setIssue("WSL 파일의 절대 경로를 입력해 주세요."); return; }
      const distro = fileDistro; const wslPath = path;
      void perform(`file:${distro}`, async () => { await call("open_wsl_file_in_log_lens", { distro, wslPath }); setPath(""); setFileDistro(null); });
    }}>
      <label>{fileDistro} 로그 파일 <input value={path} onChange={event => setPath(event.target.value)} maxLength={4096} autoComplete="off" placeholder="/var/log/example.log" /></label>
      <button disabled={busy !== null || !path}>Logs에서 열기</button><button type="button" disabled={busy !== null} onClick={() => { setFileDistro(null); setPath(""); }}>취소</button>
    </form>}
  </section>;
}
