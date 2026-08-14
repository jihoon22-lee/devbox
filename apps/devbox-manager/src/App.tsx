import { useCallback, useEffect, useState } from "react";
import { available, catalog, installApp, installed, launchApp } from "./api";
import type { CatalogApp, InstalledApp, ReleaseManifest } from "./types";
import "./App.css";

export default function App() {
  const [apps, setApps] = useState<CatalogApp[]>([]);
  const [manifest, setManifest] = useState<ReleaseManifest | null>(null);
  const [installedList, setInstalledList] = useState<InstalledApp[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setError(null);
    try {
      const [cat, av, inst] = await Promise.all([catalog(), available(), installed()]);
      setApps(cat.filter((a) => a.managerVisible));
      setManifest(av);
      setInstalledList(inst);
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

  return (
    <div className="app">
      <header className="toolbar">
        <h1 className="title">Devbox Manager</h1>
        <span className="latest">Latest: {manifest ? manifest.releaseTag : "..."}</span>
        <span className="spacer" />
        <button className="btn refresh" onClick={() => void refresh()}>
          Refresh
        </button>
      </header>

      {error && <div className="error">{error}</div>}
      {notice && <div className="notice">{notice}</div>}

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
              return (
                <tr key={a.id}>
                  <td className="app-name">{a.displayName}</td>
                  <td>{inst ? `${inst.version} (${inst.mode})` : <span className="dim">-</span>}</td>
                  <td>{app ? app.version : "-"}</td>
                  <td className="actions">
                    {inst && (
                      <button className="btn" disabled={busy === a.id} onClick={() => void onLaunch(a.id)}>
                        Launch
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

      <footer className="foot">
        <div className="dim">휴대용(portable): exe를 자체 폴더에 받아 관리. 설치 마법사(setup): 공식 설치 프로그램 실행.</div>
        <div className="dim">각 앱의 최신 버전은 release-manifest.json에서 읽는다 (release tag와 독립적).</div>
        <div className="dim">자세한 사용법: docs/windows-guide.md</div>
      </footer>
    </div>
  );
}
