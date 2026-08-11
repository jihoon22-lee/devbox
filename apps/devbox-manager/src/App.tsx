import { useCallback, useEffect, useState } from "react";
import { installApp, installed, launchApp, latest } from "./api";
import type { InstalledApp, LatestRelease } from "./types";
import "./App.css";

const APPS = [
  { crate: "port-manager", product: "PortManager", name: "Port Manager" },
  { crate: "developer-toolbox", product: "DevToolbox", name: "Developer Toolbox" },
  { crate: "wsl-dashboard", product: "WSLDashboard", name: "WSL Dashboard" },
  { crate: "api-playground", product: "ApiPlayground", name: "API Playground" },
  { crate: "activity-timeline", product: "ActivityTimeline", name: "Activity Timeline" },
  { crate: "everything-plus", product: "EverythingPlus", name: "Everything+" },
  { crate: "knowledge-base", product: "Knowledge", name: "Knowledge" },
  { crate: "life-log", product: "LifeLog", name: "Life Log" },
  { crate: "wsl-desktop", product: "WSLDesktop", name: "WSL Desktop" },
];

export default function App() {
  const [latestRel, setLatestRel] = useState<LatestRelease | null>(null);
  const [installedList, setInstalledList] = useState<InstalledApp[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setError(null);
    try {
      const [lt, inst] = await Promise.all([latest(), installed()]);
      setLatestRel(lt);
      setInstalledList(inst);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const findAsset = (appCrate: string, product: string, kind: "portable" | "installer") => {
    if (!latestRel) return null;
    const tagNoV = latestRel.tag.replace(/^v/, "");
    if (kind === "portable") {
      const a = latestRel.assets.find((x) => x.name === `${appCrate}.exe`);
      return a?.url ?? null;
    }
    const a = latestRel.assets.find((x) => x.name.startsWith(`${product}_`) && x.name.includes(`${tagNoV}_x64-setup.exe`));
    return a?.url ?? null;
  };

  const installedOf = (crate: string) => installedList.find((i) => i.app === crate);

  const onInstall = async (crate: string, url: string, mode: "portable" | "installer") => {
    if (!latestRel) return;
    setBusy(`${crate}:${mode}`);
    setError(null);
    setNotice(null);
    try {
      const msg = await installApp(crate, latestRel.tag, url, mode);
      setNotice(`${crate}: ${msg}`);
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  };

  const onLaunch = async (crate: string) => {
    setError(null);
    setNotice(null);
    try {
      await launchApp(crate);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const isUpToDate = (crate: string) => {
    const inst = installedOf(crate);
    if (!inst || !latestRel) return false;
    return inst.version === latestRel.tag.replace(/^v/, "");
  };

  return (
    <div className="app">
      <header className="toolbar">
        <h1 className="title">Devbox Manager</h1>
        <span className="latest">Latest: {latestRel ? latestRel.tag : "..."}</span>
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
            {APPS.map((a) => {
              const inst = installedOf(a.crate);
              const upToDate = isUpToDate(a.crate);
              const portableUrl = findAsset(a.crate, a.product, "portable");
              const installerUrl = findAsset(a.crate, a.product, "installer");
              return (
                <tr key={a.crate}>
                  <td className="app-name">{a.name}</td>
                  <td>{inst ? `${inst.version} (${inst.mode})` : <span className="dim">-</span>}</td>
                  <td>{latestRel ? latestRel.tag.replace(/^v/, "") : "-"}</td>
                  <td className="actions">
                    {inst && (
                      <button className="btn" disabled={busy === a.crate} onClick={() => void onLaunch(a.crate)}>
                        Launch
                      </button>
                    )}
                    {!upToDate && (
                      <>
                        {portableUrl && (
                          <button className="btn" disabled={busy === `${a.crate}:portable`} onClick={() => void onInstall(a.crate, portableUrl, "portable")}>
                            {busy === `${a.crate}:portable` ? "..." : inst ? "Update (portable)" : "Install (portable)"}
                          </button>
                        )}
                        {installerUrl && (
                          <button className="btn" disabled={busy === `${a.crate}:installer`} onClick={() => void onInstall(a.crate, installerUrl, "installer")}>
                            {busy === `${a.crate}:installer` ? "..." : inst ? "Update (setup)" : "Install (setup)"}
                          </button>
                        )}
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
        <div className="dim">자세한 사용법: docs/windows-guide.md</div>
      </footer>
    </div>
  );
}
