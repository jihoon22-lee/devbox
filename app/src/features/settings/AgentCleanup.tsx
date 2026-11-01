import { useEffect, useId, useRef, useState } from "react";
import { cleanupMessage } from "../../rpc/cleanupMessages";
import { RpcError } from "../../rpc/client";
import type { Methods } from "../../rpc/gen/rpc";
import { messageFor } from "../../rpc/messages";
import { confirmPolicy, useRpcMutation, useRpcQuery, useTopicInvalidation } from "../../rpc/query";
import { Button, ConfirmDialog } from "../../ui";
import { useCurrentProject } from "../projects/store";
import { isProjectTrustError, useProjectTrust } from "../projects/useProjectTrust";
import { QueryResult } from "./QueryResult";

type Params = Methods["agents.cleanup"]["params"];
/**
 * Next to an item the daemon lists in `offBase`: a task closed by squash or PR whose automatic
 * cleanup stopped. Its branch's own commits are not on the base branch, so "safe" in the list's
 * heading holds for it only with this said.
 */
const OFF_BASE = "스쿼시·PR로 닫힘 — 브랜치의 개별 커밋은 기준 브랜치에 없습니다";
/**
 * The daemon lists apart (`submodules`) a closed task's work folder that passes every check but
 * holds initialized submodules: Git removes it only with force, which devbox never uses. It is
 * shown with how to remove it, outside the safe items, and cannot be selected.
 */
const KEPT = "직접 지울 작업 폴더";
export function AgentCleanup({ projectId }: { projectId: string }) {
  const candidates = useRpcQuery("agents.cleanup_candidates", { projectId });
  const offBase = new Set(candidates.data?.offBase);
  const noteId = useId();
  useTopicInvalidation("agents.changed", [["agents.cleanup_candidates", { projectId }]]);
  const cleanup = useRpcMutation("agents.cleanup");
  const trust = useProjectTrust(projectId);
  const [selected, setSelected] = useState<Params>({ projectId, worktrees: [], branches: [] });
  const [snapshot, setSnapshot] = useState<Params>();
  const [notice, setNotice] = useState("");
  const [preparing, setPreparing] = useState(false);
  const [needsRecheck, setNeedsRecheck] = useState(false);
  const busy = useRef(false),
    mounted = useRef(true),
    version = useRef(0);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      version.current++;
    };
  }, []);
  const current = () => mounted.current && useCurrentProject.getState().id === projectId;
  const selection = () => ({
    projectId,
    worktrees: selected.worktrees.filter((path) => candidates.data?.worktrees.includes(path)),
    branches: selected.branches.filter((branch) => candidates.data?.branches.includes(branch)),
  });
  const prepare = async (targets: Params) => {
    if (busy.current || !current()) return;
    busy.current = true;
    setPreparing(true);
    setSnapshot(undefined);
    const stamp = ++version.current;
    try {
      if (
        targets.worktrees.length &&
        !(await trust.ensure({
          id: projectId,
          operation: "cleanup",
          agentId: null,
          worktrees: targets.worktrees,
          worktree: null,
        }))
      )
        return;
      if (!current() || stamp !== version.current) return;
      const fresh = await candidates.refetch();
      if (!current() || stamp !== version.current) return;
      if (fresh.error) throw fresh.error;
      if (
        !fresh.data ||
        targets.worktrees.some((path) => !fresh.data.worktrees.includes(path)) ||
        targets.branches.some((branch) => !fresh.data.branches.includes(branch))
      ) {
        setNeedsRecheck(false);
        setNotice("정리 후보가 바뀌었습니다. 현재 목록에서 삭제할 항목을 다시 선택하세요.");
        return;
      }
      cleanup.reset();
      setNeedsRecheck(false);
      setNotice("");
      setSnapshot({ ...targets, worktrees: [...targets.worktrees], branches: [...targets.branches] });
    } catch (error) {
      if (!current() || stamp !== version.current) return;
      const failure = error instanceof RpcError ? error : new RpcError("internal");
      const message = messageFor(failure.code, failure.detail);
      setNotice(`${message.title} ${message.action ?? ""}`);
    } finally {
      busy.current = false;
      if (current()) setPreparing(false);
    }
  };
  const open = () => {
    if (busy.current || !current() || candidates.isPending || candidates.error) return;
    if (confirmPolicy("agents.cleanup") !== "always") throw new Error("Cleanup requires confirmation");
    const targets = selection();
    if (!targets.worktrees.length && !targets.branches.length) return;
    void prepare(targets);
  };
  const confirm = async () => {
    if (busy.current || !current() || !snapshot || needsRecheck) return;
    busy.current = true;
    try {
      await cleanup.mutateAsync(snapshot);
      if (!current()) return;
      setSnapshot(undefined);
      setSelected({ projectId, worktrees: [], branches: [] });
      setNotice("선택 항목을 정리했습니다");
      void candidates.refetch();
    } catch (error) {
      if (!current()) return;
      const failure = error instanceof RpcError ? error : new RpcError("internal");
      const message = cleanupMessage(failure.code, failure.detail, "list");
      setNotice(
        `${message.title} ${message.action ?? ""}${failure.diagnosticId ? ` · 진단 ${failure.diagnosticId}` : ""}`,
      );
      if (isProjectTrustError(failure)) setNeedsRecheck(true);
    } finally {
      busy.current = false;
    }
  };
  const cancel = () => {
    if (cleanup.isPending) return;
    version.current++;
    trust.cancel();
    cleanup.reset();
    setSnapshot(undefined);
    setNeedsRecheck(false);
    setNotice("");
  };
  const toggle = (kind: "worktrees" | "branches", value: string, checked: boolean) => {
    if (busy.current || snapshot) return;
    setSelected((previous) => ({
      ...previous,
      [kind]: checked ? [...new Set([...previous[kind], value])] : previous[kind].filter((item) => item !== value),
    }));
  };
  const available = selection();
  return (
    <section aria-label="정리 후보">
      <h3>현재 프로젝트의 정리 후보</h3>
      <p>닫힌 작업이 소유한 안전한 항목만 표시합니다. 삭제할 작업 폴더와 브랜치를 선택하세요.</p>
      <QueryResult pending={candidates.isPending} error={candidates.error} retry={() => void candidates.refetch()}>
        {(["worktrees", "branches"] as const).map((kind) => (
          <fieldset key={kind} disabled={cleanup.isPending || preparing || !!snapshot}>
            <legend>{kind === "worktrees" ? "작업 폴더" : "브랜치"}</legend>
            {candidates.data?.[kind].map((value, index) => {
              const note = offBase.has(value) ? `${noteId}-${kind}-${index}` : undefined;
              return (
                <p key={value}>
                  <label>
                    <input
                      type="checkbox"
                      aria-describedby={note}
                      checked={available[kind].includes(value)}
                      onChange={(event) => toggle(kind, value, event.target.checked)}
                    />{" "}
                    {value}
                  </label>
                  {note && <small id={note}> {OFF_BASE}</small>}
                </p>
              );
            })}
          </fieldset>
        ))}
        {candidates.data && !candidates.data.worktrees.length && !candidates.data.branches.length && (
          <p>정리할 항목이 없습니다</p>
        )}
        {!!candidates.data?.submodules?.length && (
          <section aria-label={KEPT}>
            <h4>{KEPT}</h4>
            {candidates.data.submodules.map((path) => {
              const message = messageFor("agents.worktree_has_submodules", { path });
              return (
                <p key={path}>
                  {path}
                  <small>
                    {" "}
                    {message.title} {message.action}
                  </small>
                </p>
              );
            })}
          </section>
        )}
      </QueryResult>
      <Button
        variant="danger"
        disabled={
          cleanup.isPending ||
          preparing ||
          candidates.isPending ||
          !!candidates.error ||
          (!available.worktrees.length && !available.branches.length)
        }
        onClick={open}
      >
        선택 항목 정리
      </Button>
      {preparing && <p role="status">정리할 항목을 다시 확인하는 중…</p>}
      {!snapshot && notice && <p role="status">{notice}</p>}
      {trust.notice && <p role="alert">{trust.notice}</p>}
      <ConfirmDialog
        open={!!snapshot}
        title="선택 항목 정리"
        body={`현재 프로젝트의 작업 폴더 ${snapshot?.worktrees.length ?? 0}개와 브랜치 ${snapshot?.branches.length ?? 0}개를 삭제합니다.`}
        confirmLabel="정리"
        disabled={cleanup.isPending || needsRecheck}
        onConfirm={() => void confirm()}
        onCancel={cancel}
      >
        <ul>
          {[...(snapshot?.worktrees ?? []), ...(snapshot?.branches ?? [])].map((value) => (
            <li key={value}>
              {value}
              {offBase.has(value) && <small> {OFF_BASE}</small>}
            </li>
          ))}
        </ul>
        {notice && <p role="alert">{notice}</p>}
        {needsRecheck && <Button onClick={() => snapshot && void prepare(snapshot)}>다시 확인</Button>}
      </ConfirmDialog>
      {trust.dialog}
    </section>
  );
}
