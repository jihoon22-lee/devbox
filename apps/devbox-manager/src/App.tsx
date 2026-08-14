import { useCallback, useEffect, useState } from "react";
import { available, catalog, current, installApp, installed, launchApp, rollback, runDiagnosis, type DiagnosisItem } from "./api";
import type { CatalogApp, Current, InstalledApp, ReleaseManifest } from "./types";
import "./App.css";

export default function App() {
  const [apps, setApps] = useState<CatalogApp[]>([]);
  const [manifest, setManifest] = useState<ReleaseManifest | null>(null);
  const [installedList, setInstalledList] = useState<InstalledApp[]>([]);
  const [currentMap, setCurrentMap] = useState<Record<string, Current | null>>({});
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [tab, setTab] = useState<"apps" | "doctor">("apps");
  const [diagnosis, setDiagnosis] = useState<DiagnosisItem[]>([]);

  const onDiagnose = useCallback(async () => {
    setError(null);
    try {
      setDiagnosis(await runDiagnosis());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  const refresh = useCallback(async () => {
    setError(null);
    try {
      const [cat, av, inst] = await Promise.all([catalog(), available(), installed()]);
      setApps(cat.filter((a) => a.managerVisible));
      setManifest(av);
      setInstalledList(inst);
      // portable로 설치된 앱의 current.json 정보
      const curMap: Record<string, Current | null> = {};
      for (const i of inst) {
        if (i.mode === "portable") {
          curMap[i.app] = await current(i.app);
        }
      }
      setCurrentMap(curMap);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const manifestOf = (appId: string) => manifest?.apps.find((a) => a.id === appId);
  const installedOf = (appId: string) => installedList.find((i) => i.app === appId);

  const isUpToDate = (appId: string) => {
    const inst = installedOf(appId);
    const app = manifestOf(appId);
    if (!inst || !app) return false;
    return inst.version === app.version;
  };

  const onInstall = async (appId: string, mode: "portable" | "installer") => {
    setBusy(`${appId}:${mode}`);
    setError(null);
    setNotice(null);
    try {
      const msg = await installApp(appId, mode);
      setNotice(`${appId}: ${msg}`);
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  };

  const onLaunch = async (appId: string) => {
    setError(null);
    setNotice(null);
    try {
      await launchApp(appId);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onRollback = async (appId: string) => {
    setBusy(appId);
    setError(null);
    setNotice(null);
    try {
      const msg = await rollback(appId);
      setNotice(msg);
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="app">
      <header className="toolbar">
        <h1 className="title">Devbox Manager</h1>
        <button className={`btn ${tab === "apps" ? "active" : ""}`} onClick={() => setTab("apps")}>
          앱
        </button>
        <button className={`btn ${tab === "doctor" ? "active" : ""}`} onClick={() => { setTab("doctor"); void onDiagnose(); }}>
          환경 진단
        </button>
        <span className="latest">Latest: {manifest ? manifest.releaseTag : "..."}</span>
        <span className="spacer" />
        <button className="btn refresh" onClick={() => void refresh()}>
          Refresh
        </button>
      </header>

      {error && <div className="error">{error}</div>}
      {notice && <div className="notice">{notice}</div>}

      {tab === "doctor" ? (
        <div className="doctor">
          <div className="doctor-head">
            <span className="dim">read-only 진단 · 자동 설치·수정 없음</span>
            <button className="btn" onClick={() => void onDiagnose()}>다시 진단</button>
          </div>
          {diagnosis.map((d) => (
            <div key={d.name} className={`doctor-row ${d.ok ? "ok" : "bad"}`}>
              <span className="doctor-name">{d.name}</span>
              <span className="doctor-detail">{d.detail}</span>
            </div>
          ))}
          {diagnosis.length === 0 && <div className="dim">진단을 실행해 주세요.</div>}
          <div className="dim doctor-note">지원 번들·path·환경변수는 redaction되어야 합니다 (§15.4 경계).</div>
        </div>
      ) : (
      <div className="table-wrap">
        <table>
          <thead>
            <tr>
              <th>APP</th>
              <th>INSTALLED</th>
              <th>LATEST</th>
              <th>ACTION</th>
            </tr>
          </thead>
          <tbody>
            {apps.map((a) => {
              const inst = installedOf(a.id);
              const app = manifestOf(a.id);
              const upToDate = isUpToDate(a.id);
              const cur = currentMap[a.id];
              const canRollback = inst?.mode === "portable" && cur?.previousVersion != null;
              return (
                <tr key={a.id}>
                  <td className="app-name">{a.displayName}</td>
                  <td>
                    {inst ? (
                      <span>
                        {inst.version} ({inst.mode})
                        {canRollback && (
                          <span className="dim"> ← prev {cur?.previousVersion}</span>
                        )}
                      </span>
                    ) : (
                      <span className="dim">-</span>
                    )}
                  </td>
                  <td>{app ? app.version : "-"}</td>
                  <td className="actions">
                    {inst && (
                      <button className="btn" disabled={busy === a.id} onClick={() => void onLaunch(a.id)}>
                        Launch
                      </button>
                    )}
                    {canRollback && (
                      <button className="btn" disabled={busy === a.id} onClick={() => void onRollback(a.id)}>
                        {busy === a.id ? "..." : "Rollback"}
                      </button>
                    )}
                    {!upToDate && app && (
                      <>
                        <button className="btn" disabled={busy === `${a.id}:portable`} onClick={() => void onInstall(a.id, "portable")}>
                          {busy === `${a.id}:portable` ? "..." : inst ? "Update (portable)" : "Install (portable)"}
                        </button>
                        <button className="btn" disabled={busy === `${a.id}:installer`} onClick={() => void onInstall(a.id, "installer")}>
                          {busy === `${a.id}:installer` ? "..." : inst ? "Update (setup)" : "Install (setup)"}
                        </button>
                      </>
                    )}
                    {upToDate && <span className="dim tag">up to date</span>}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      )}

      <footer className="foot">
        <div className="dim">휴대용(portable): exe를 자체 폴더에 받아 관리. 설치 마법사(setup): 공식 설치 프로그램 실행.</div>
        <div className="dim">각 앱의 최신 버전은 release-manifest.json에서 읽는다 (release tag와 독립적).</div>
        <div className="dim">자세한 사용법: docs/windows-guide.md</div>
      </footer>
    </div>
  );
}
