import { useEffect, useRef, useState } from "react";
import { RpcError } from "../../rpc/client";
import type { Methods } from "../../rpc/gen/rpc";
import { messageFor } from "../../rpc/messages";
import { confirmPolicy, useRpcMutation, useRpcQuery, useTopicInvalidation } from "../../rpc/query";
import { Button, ConfirmDialog } from "../../ui";
import { useCurrentProject } from "../projects/store";
import { QueryResult } from "./QueryResult";

type Params = Methods["agents.cleanup"]["params"];
export function AgentCleanup({ projectId }: { projectId: string }) {
  const candidates = useRpcQuery("agents.cleanup_candidates", { projectId });
  useTopicInvalidation("agents.changed", [["agents.cleanup_candidates", { projectId }]]);
  const cleanup = useRpcMutation("agents.cleanup");
  const [selected, setSelected] = useState<Params>({ projectId, worktrees: [], branches: [] });
  const [snapshot, setSnapshot] = useState<Params>();
  const [notice, setNotice] = useState("");
  const busy = useRef(false),
    mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  const current = () => mounted.current && useCurrentProject.getState().id === projectId;
  const selection = () => ({
    projectId,
    worktrees: selected.worktrees.filter((path) => candidates.data?.worktrees.includes(path)),
    branches: selected.branches.filter((branch) => candidates.data?.branches.includes(branch)),
  });
  const open = () => {
    if (busy.current || !current() || candidates.isPending || candidates.error) return;
    if (confirmPolicy("agents.cleanup") !== "always") throw new Error("Cleanup requires confirmation");
    const selected = selection();
    if (!selected.worktrees.length && !selected.branches.length) return;
    cleanup.reset();
    setNotice("");
    setSnapshot(selected);
  };
  const confirm = async () => {
    if (busy.current || !current() || !snapshot) return;
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
      const message = messageFor(failure.code, failure.detail);
      setNotice(
        `${message.title} ${message.action ?? ""}${failure.diagnosticId ? ` · 진단 ${failure.diagnosticId}` : ""}`,
      );
    } finally {
      busy.current = false;
    }
  };
  const cancel = () => {
    if (busy.current) return;
    cleanup.reset();
    setSnapshot(undefined);
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
          <fieldset key={kind} disabled={cleanup.isPending || !!snapshot}>
            <legend>{kind === "worktrees" ? "작업 폴더" : "브랜치"}</legend>
            {candidates.data?.[kind].map((value) => (
              <p key={value}>
                <label>
                  <input
                    type="checkbox"
                    checked={available[kind].includes(value)}
                    onChange={(event) => toggle(kind, value, event.target.checked)}
                  />{" "}
                  {value}
                </label>
              </p>
            ))}
          </fieldset>
        ))}
        {candidates.data && !candidates.data.worktrees.length && !candidates.data.branches.length && (
          <p>정리할 항목이 없습니다</p>
        )}
      </QueryResult>
      <Button
        variant="danger"
        disabled={
          cleanup.isPending ||
          candidates.isPending ||
          !!candidates.error ||
          (!available.worktrees.length && !available.branches.length)
        }
        onClick={open}
      >
        선택 항목 정리
      </Button>
      {!snapshot && notice && <p role="status">{notice}</p>}
      <ConfirmDialog
        open={!!snapshot}
        title="선택 항목 정리"
        body={`현재 프로젝트의 작업 폴더 ${snapshot?.worktrees.length ?? 0}개와 브랜치 ${snapshot?.branches.length ?? 0}개를 삭제합니다.`}
        confirmLabel="정리"
        disabled={cleanup.isPending}
        onConfirm={() => void confirm()}
        onCancel={cancel}
      >
        <ul>
          {[...(snapshot?.worktrees ?? []), ...(snapshot?.branches ?? [])].map((value) => (
            <li key={value}>{value}</li>
          ))}
        </ul>
        {notice && <p role="alert">{notice}</p>}
      </ConfirmDialog>
    </section>
  );
}
