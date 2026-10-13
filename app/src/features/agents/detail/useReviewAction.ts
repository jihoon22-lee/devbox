import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { RpcError } from "../../../rpc/client";
import type { MergeKind, Methods } from "../../../rpc/gen/rpc";
import { messageFor } from "../../../rpc/messages";
import { confirmPolicy, useRpcMutation, useRpcQuery } from "../../../rpc/query";
import { useToasts } from "../../../ui";
import type { AgentTask } from "../model";
import { useAgentResult } from "./data";
import { openAgentPr } from "./links";

type Changes = Methods["agents.changes"]["result"];
type Check = Methods["agents.discard_check"]["result"];
type Intent =
  | { kind: "merge"; strategy: MergeKind; message: string | null }
  | { kind: "pr"; title: string; body: string }
  | { kind: "discard" };
type Snapshot = { intent: Intent; changes?: Changes; check?: Check };
export function useReviewAction(task: AgentTask, onConflict: (files: string[]) => void) {
  const changes = useRpcQuery("agents.changes", { id: task.id }, { enabled: false });
  const check = useRpcQuery("agents.discard_check", { id: task.id }, { enabled: false });
  const merge = useRpcMutation("agents.merge"),
    pr = useRpcMutation("agents.pr"),
    discard = useRpcMutation("agents.discard");
  const apply = useAgentResult(),
    qc = useQueryClient();
  const [snapshot, setSnapshot] = useState<Snapshot>();
  const [notice, setNotice] = useState("");
  const [loading, setLoading] = useState(false);
  const guard = useRef(false),
    version = useRef(0),
    mismatch = useRef(false);
  // biome-ignore lint/correctness/useExhaustiveDependencies: A different task must cancel the previous confirmation snapshot.
  useEffect(() => {
    version.current++;
    mismatch.current = false;
    setSnapshot(undefined);
    setNotice("");
    setLoading(false);
    return () => {
      version.current++;
    };
  }, [task.id]);
  const errorText = (error: RpcError) => {
    const message = messageFor(error.code, error.detail);
    return `${message.title} ${message.action ?? ""}${error.diagnosticId ? ` · 진단 ${error.diagnosticId}` : ""}`;
  };
  const refresh = async (intent: Intent, stamp: number) => {
    setLoading(true);
    const keys = [
      ["agents.changes", { id: task.id }],
      ["agents.commits", { id: task.id }],
      ["agents.patch", { id: task.id }],
    ];
    const details: Promise<void>[] = [];
    if (mismatch.current) {
      // External Git changes can be refused before any agents.changed event.
      // Mark all this task's review data stale, including inactive cached files.
      await Promise.all(keys.map((queryKey) => qc.invalidateQueries({ queryKey, refetchType: "none" })));
      for (const queryKey of keys) {
        // The merge/PR snapshot refetch below already refreshes agents.changes.
        if (queryKey[0] === "agents.changes" && intent.kind !== "discard") continue;
        details.push(qc.refetchQueries({ queryKey, type: "active" }, { throwOnError: true }));
      }
    }
    const [response, reviewed] = await Promise.all([
      intent.kind === "discard" ? check.refetch() : changes.refetch(),
      Promise.allSettled(details),
    ]);
    if (stamp !== version.current) return;
    setLoading(false);
    const failed = reviewed.find((result) => result.status === "rejected");
    if (response.error || !response.data || failed) {
      const error =
        response.error ??
        (failed?.status === "rejected" && failed.reason instanceof RpcError ? failed.reason : undefined);
      setNotice(
        `${mismatch.current ? "확인한 뒤 작업이 바뀌었습니다. 최신 검토 내용을 확인하지 못했습니다. " : ""}${error ? errorText(error) : "상태를 확인하지 못했습니다. 다시 확인하세요."}`,
      );
      return false;
    }
    // Only enable the new frozen snapshot once every visible review query succeeds.
    setSnapshot(
      intent.kind === "discard"
        ? { intent, check: { ...(response.data as Check) } }
        : { intent, changes: { ...(response.data as Changes) } },
    );
    return true;
  };
  const open = async (intent: Intent) => {
    if (guard.current || loading || task.state === "closed") return;
    const method = intent.kind === "merge" ? "agents.merge" : intent.kind === "pr" ? "agents.pr" : "agents.discard";
    if (confirmPolicy(method) !== "always") throw new Error("Review action requires confirmation");
    mismatch.current = false;
    setNotice("");
    setSnapshot({ intent });
    await refresh(intent, version.current);
  };
  const confirm = async () => {
    if (guard.current || loading || !snapshot || (!snapshot.check && !snapshot.changes)) return;
    guard.current = true;
    const stamp = version.current;
    const { intent, changes, check } = snapshot;
    try {
      const result =
        intent.kind === "discard"
          ? await discard.mutateAsync({
              id: task.id,
              expect: {
                headOid: check!.headOid,
                unmergedCommits: check!.unmergedCommits,
                discardFingerprint: check!.fingerprint ?? undefined,
              },
            })
          : intent.kind === "merge"
            ? await merge.mutateAsync({
                id: task.id,
                strategy: intent.strategy,
                message: intent.message,
                expect: { headOid: changes!.headOid, baseOid: changes!.baseOid },
              })
            : await pr.mutateAsync({
                id: task.id,
                title: intent.title,
                body: intent.body,
                expect: { headOid: changes!.headOid, baseOid: changes!.baseOid },
              });
      apply(result);
      if (intent.kind === "pr" && result.prUrl) {
        const url = result.prUrl;
        useToasts.getState().push({
          tone: "ok",
          title: "PR을 만들었습니다",
          body: url,
          action: { label: "PR 열기", run: () => openAgentPr(url) },
        });
      }
      if (stamp !== version.current) return;
      setSnapshot(undefined);
      setNotice("");
    } catch (error) {
      if (stamp !== version.current) return;
      const failure = error instanceof RpcError ? error : new RpcError("internal");
      setNotice(errorText(failure));
      if (failure.code === "agents.state_changed") {
        // Remove the old snapshot while refreshing. No mutation is retried here.
        setSnapshot({ intent });
        mismatch.current = true;
        await refresh(intent, stamp);
      } else if (failure.code === "agents.conflict") {
        const detail = failure.detail as { files?: string[] } | undefined;
        onConflict(detail?.files ?? []);
        setSnapshot(undefined);
      }
    } finally {
      guard.current = false;
    }
  };
  const cancel = () => {
    if (guard.current) return;
    version.current++;
    mismatch.current = false;
    setSnapshot(undefined);
    setLoading(false);
    setNotice("");
  };
  const inline = !task.worktreePath;
  const label =
    snapshot?.intent.kind === "discard"
      ? inline
        ? "세션 끝내기"
        : "작업 폴더와 브랜치 삭제"
      : snapshot?.intent.kind === "pr"
        ? "PR 만들기"
        : `${snapshot?.changes?.base ?? task.baseBranch}에 병합`;
  const body =
    snapshot?.intent.kind === "discard"
      ? inline
        ? `이 에이전트 작업의 세션을 끝냅니다.${snapshot.check ? ` HEAD ${snapshot.check.headOid} · 병합하지 않은 커밋 ${snapshot.check.unmergedCommits}개` : " 현재 상태 확인 중…"}`
        : snapshot.check
          ? `${snapshot.check.unmergedCommits === 0 && snapshot.check.dirtyFiles === 0 ? "작업 폴더와 브랜치를 지웁니다." : `병합하지 않은 커밋 ${snapshot.check.unmergedCommits}개와 커밋하지 않은 파일 ${snapshot.check.dirtyFiles}개가 사라집니다.`} HEAD ${snapshot.check.headOid}`
          : "현재 상태 확인 중…"
      : snapshot?.changes
        ? `기준 ${snapshot.changes.base} · 브랜치 ${task.branch} · 앞섬 ${snapshot.changes.ahead} · 뒤처짐 ${snapshot.changes.behind}\nHEAD ${snapshot.changes.headOid} · 기준 ${snapshot.changes.baseOid}${snapshot.intent.kind === "pr" ? `\nPR 제목: ${snapshot.intent.title}\n${snapshot.intent.body}` : snapshot.intent.message ? `\n커밋 메시지: ${snapshot.intent.message}` : ""}`
        : "현재 상태 확인 중…";
  return {
    snapshot,
    open,
    confirm,
    cancel,
    label,
    body: `${task.title}\n${body}${notice ? `\n${notice}` : ""}`,
    busy: merge.isPending || pr.isPending || discard.isPending,
    blocked: loading || !snapshot || (!snapshot.check && !snapshot.changes),
    loading,
    notice,
    retry: () => snapshot && refresh(snapshot.intent, version.current),
  };
}
