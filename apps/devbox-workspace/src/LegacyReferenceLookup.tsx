import {useEffect, useRef, useState} from "react";
import type {ProjectContext} from "@devbox/product-shell/api";
import type {Registry} from "./RegistryGate";
import type {ImportedProfile} from "./LegacyProfileImport";
import {nativeCall} from "./native";

interface Resolution {
  schemaVersion: number;
  registryRevision: number;
  state: "unmapped" | "resolved" | "ambiguous";
  candidates: {context: ProjectContext; origins: {kind: string; importedId?: string}[]}[];
}
interface Props {
  registry: Registry;
  imported: ImportedProfile;
  disabled: boolean;
  onSelect: (context: ProjectContext) => void;
}
export default function LegacyReferenceLookup({registry, imported, disabled, onSelect}: Props) {
  const [specific, setSpecific] = useState(false);
  const [result, setResult] = useState<Resolution | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const generation = useRef(0);
  const pending = useRef(false);
  useEffect(() => () => {generation.current += 1;}, []);
  async function lookup() {
    if (pending.current || disabled) return;
    pending.current = true; setBusy(true); setResult(null); setError("");
    const current = generation.current;
    try {
      const value = await nativeCall<Resolution>("workspace.registry", "resolve_legacy_reference", {
        registryRevision: registry.revision, owner: "workbench", oldId: imported.profile.id,
        target: null, importedId: specific ? imported.id : null,
      });
      if (current === generation.current) {
        if (value.schemaVersion !== 1 || value.registryRevision !== registry.revision) throw new Error("목록이 변경되었습니다. 새로 고친 뒤 다시 확인하세요.");
        setResult(value);
      }
    } catch (cause) {
      if (current === generation.current) setError(cause instanceof Error ? cause.message : "연결을 확인하지 못했습니다.");
    } finally {
      pending.current = false;
      if (current === generation.current) setBusy(false);
    }
  }
  return <section aria-label="기존 프로필 참조 연결" aria-busy={busy}>
    <p>기존 프로필 ID: {imported.profile.id}</p>
    <p>같은 ID로 보관된 프로필과 Windows·WSL 연결을 함께 확인합니다. 연결 조회는 폴더나 서버를 실행하지 않습니다.</p>
    <label><input type="checkbox" checked={specific} disabled={busy || disabled} onChange={event => {setSpecific(event.currentTarget.checked); setResult(null);}}/>이 보관 항목만 확인</label>
    <button disabled={busy || disabled} onClick={() => void lookup()}>참조 연결 조회</button>
    {error && <p role="alert">{error}</p>}
    {result && <>
      <p role="status">{result.state === "unmapped" ? "연결된 프로젝트가 없습니다. 프로필과 기존 ID는 보존됩니다."
        : result.state === "ambiguous" ? "여러 프로젝트가 연결되어 있습니다. 사용할 폴더를 직접 선택하세요."
          : "연결된 프로젝트를 확인했습니다. 사용할 때 선택하세요."}</p>
      {result.candidates.map(candidate => {
        const tree = registry.worktrees.find(tree => tree.id === candidate.context.worktreeId && tree.projectId === candidate.context.projectId && tree.revision === candidate.context.revision);
        return tree && <div key={tree.id}>
          <p>{tree.binding.target.kind === "windows" ? "Windows" : "WSL"}: {tree.binding.root}</p>
          <button disabled={disabled || busy} onClick={() => onSelect(candidate.context)}>이 연결의 프로젝트 선택</button>
        </div>;
      })}
    </>}
  </section>;
}
