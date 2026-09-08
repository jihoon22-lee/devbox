import {useCallback, useEffect, useMemo, useState} from "react";
import {repoStatus, worktrees, type RepoEntry, type RepoSnapshot} from "./api";
import GitSafetyPanel from "./components/GitSafetyPanel";
import HistoryDiffPanel from "./components/HistoryDiffPanel";
import StageCommitPanel from "./components/StageCommitPanel";
import RemoteSyncPanel from "./components/RemoteSyncPanel";
import CleanupPanel from "./components/CleanupPanel";
import {WorkspaceOperationError} from "../transport";
import "./App.css";

interface Props {repo:RepoEntry; onBusyChange:(busy:boolean)=>void; onDirtyChange:(dirty:boolean)=>void}
/** Product composition consumes the selected native Registry projection. */
export default function SourcePanel({repo,onBusyChange,onDirtyChange}:Props) {
  const [busy,setBusy]=useState(false);
  const [panels,setPanels]=useState<Record<string,boolean>>({});
  const [snapshot,setSnapshot]=useState<RepoSnapshot|null>(null);
  const [trees,setTrees]=useState<string[]>([]);
  const [error,setError]=useState("");
  const callbacks=useMemo(()=>Object.fromEntries(["safety","history","stage","remote","cleanup"].map(name=>[name,(value:boolean)=>setPanels(previous=>previous[name]===value?previous:{...previous,[name]:value})])),[]);
  useEffect(()=>{onBusyChange(busy||Object.values(panels).some(Boolean));},[busy,panels,onBusyChange]);
  useEffect(()=>()=>onBusyChange(false),[onBusyChange]);
  const refresh=useCallback(async()=>{
    if(busy)return;
    setBusy(true);setError("");
    try {const [next,paths]=await Promise.all([repoStatus(repo.path),worktrees(repo.path)]);setSnapshot(next);setTrees(paths);}
    catch(cause){setError(cause instanceof WorkspaceOperationError?cause.message:"저장소 상태를 불러오지 못했습니다.");}
    finally{setBusy(false);}
  },[busy,repo.path]);
  return <div className="workspace-native-source-panels">
    <section aria-label="현재 저장소">
      <h2>현재 저장소</h2><p>{repo.path}</p>
      <button type="button" disabled={busy} onClick={()=>void refresh()}>저장소 상태 새로 고침</button>
      {error&&<p role="alert">{error}</p>}
      {snapshot&&<p>{snapshot.branch.current} · 변경 {snapshot.changes}개 · 앞섬 {snapshot.branch.ahead} / 뒤처짐 {snapshot.branch.behind}</p>}
      {trees.length>0&&<details><summary>연결된 작업 폴더 {trees.length}개</summary><ul>{trees.map(path=><li key={path}>{path}</li>)}</ul></details>}
    </section>
    <GitSafetyPanel repo={repo} onBusyChange={callbacks.safety}/>
    <HistoryDiffPanel repo={repo} onBusyChange={callbacks.history}/>
    <StageCommitPanel repo={repo} onBusyChange={callbacks.stage} onDirtyChange={onDirtyChange}/>
    <RemoteSyncPanel repo={repo} onBusyChange={callbacks.remote}/>
    <CleanupPanel repo={repo} onBusyChange={callbacks.cleanup}/>
  </div>;
}
