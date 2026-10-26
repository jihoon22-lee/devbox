import { useEffect, useRef, useState } from "react";
import { readAgentList } from "../../rpc/agentPages";
import type { RpcClient } from "../../rpc/client";
import { useRpcSession } from "../../rpc/RpcProvider";
import { Button, Dialog } from "../../ui";
import { type InstallSummary, readInstallSummary } from "./nativeUpdate";

interface Counts {
  agents: number;
  open: number;
  services: number;
  active: number;
}
const states = ["preparing", "running", "waiting", "idle", "failed", "review", "closed"];
const live = ["running", "waiting", "idle", "review"];
async function executionCounts(client: RpcClient, signal: AbortSignal): Promise<Counts> {
  const [agents, runtime] = await Promise.all([
    readAgentList(client, { includeClosedHours: 0 }, signal),
    client.call("runtime.summary", {}, { signal }),
  ]);
  if (
    !agents.items.every((task) => states.includes(task.state) && typeof task.toolExited === "boolean") ||
    !Number.isSafeInteger(runtime.active) ||
    runtime.active < 0 ||
    !Number.isSafeInteger(runtime.activeServices) ||
    runtime.activeServices < 0 ||
    runtime.activeServices > runtime.active
  ) {
    throw new Error("Invalid execution counts");
  }
  return {
    agents: agents.items.filter((task) => !task.toolExited && live.includes(task.state)).length,
    open: agents.items.filter((task) => task.state !== "closed").length,
    services: runtime.activeServices,
    active: runtime.active,
  };
}
function blockedMessage(summary: InstallSummary) {
  switch (summary.blocked) {
    case "nativeActive":
      return `앱에서 실행한 Windows 작업 ${summary.nativeActive ?? "여러"}개가 진행 중입니다. 완료를 기다리거나 실행 화면에서 직접 중지한 뒤 다시 조회하세요.`;
    case "nativeUnknown":
      return "Windows 작업 상태를 확인하지 못했습니다. 실행 상태를 확인하고 다시 조회하세요.";
    case "notReady":
      return "업데이트 파일 상태가 바뀌었습니다. 상태를 다시 확인하세요.";
    case "busy":
      return "다른 업데이트 준비가 진행 중입니다. 완료되거나 취소된 뒤 다시 조회하세요.";
    case null:
      return null;
  }
}
interface View {
  native: InstallSummary | null;
  counts: Counts | null;
  nativeProblem: boolean;
  countsProblem: boolean;
  loading: boolean;
}
const loading: View = { native: null, counts: null, nativeProblem: false, countsProblem: false, loading: true };

export function UpdateConfirmation({
  generation,
  version,
  busy,
  onConfirm,
  onCancel,
}: {
  generation: number;
  version: string;
  busy: boolean;
  onConfirm: (generation: number, confirmationId: number) => Promise<void>;
  onCancel: () => void;
}) {
  const session = useRpcSession();
  const [view, setView] = useState<View>(loading);
  const [attempt, setAttempt] = useState(0);
  const alive = useRef(false);
  const querying = useRef(true);
  const confirming = useRef(false);
  const cancel = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  // biome-ignore lint/correctness/useExhaustiveDependencies: an explicit retry refreshes both informational counts and the native confirmation ticket.
  useEffect(() => {
    const controller = new AbortController();
    let current = true;
    querying.current = true;
    setView(loading);
    const counts =
      session.state === "ready"
        ? executionCounts(session.client, controller.signal)
        : Promise.reject(new Error("Daemon unavailable"));
    void Promise.allSettled([readInstallSummary(generation), counts]).then(([native, daemon]) => {
      if (!current) return;
      querying.current = false;
      const valid =
        native.status === "fulfilled" &&
        native.value.generation === generation &&
        (native.value.blocked !== null || native.value.version === version);
      setView({
        native: valid ? native.value : null,
        counts: daemon.status === "fulfilled" ? daemon.value : null,
        nativeProblem: !valid,
        countsProblem: daemon.status === "rejected",
        loading: false,
      });
    });
    return () => {
      current = false;
      controller.abort();
    };
  }, [generation, version, session.client, session.generation, session.state, attempt]);
  const enabled =
    !!view.native &&
    !!view.counts &&
    view.native.blocked === null &&
    view.native.confirmationId !== null &&
    !view.loading &&
    !busy;
  const confirm = async () => {
    if (!enabled || querying.current || confirming.current || view.native?.confirmationId == null) return;
    confirming.current = true;
    try {
      await onConfirm(view.native.generation, view.native.confirmationId);
    } finally {
      confirming.current = false;
      if (alive.current) onCancel();
    }
  };
  const retry = () => {
    querying.current = true;
    setView(loading);
    setAttempt((value) => value + 1);
  };
  return (
    <Dialog
      open
      title="업데이트 설치"
      description="앱을 닫고 업데이트를 설치한 뒤 다시 시작합니다. WSL의 에이전트 작업과 서비스는 계속 실행됩니다."
      onOpenChange={(open) => {
        if (!open && !busy && !confirming.current) onCancel();
      }}
      onOpenAutoFocus={(event) => {
        event.preventDefault();
        cancel.current?.focus();
      }}
      footer={
        <>
          <Button
            ref={cancel}
            disabled={busy}
            onClick={() => {
              if (!confirming.current) onCancel();
            }}
          >
            취소
          </Button>
          <Button variant="primary" disabled={!enabled} onClick={() => void confirm()}>
            설치하고 다시 시작
          </Button>
        </>
      }
    >
      <p>설치할 버전 {version}</p>
      <p>앱에서 시작한 Windows 작업은 설치 전에 완료하거나 실행 화면에서 직접 중지해야 합니다.</p>
      {view.loading && <p role="status">설치 전 상태를 확인하고 있습니다.</p>}
      {view.counts && (
        <>
          <p>
            에이전트 작업: 실행 중 {view.counts.agents}개 · 열린 에이전트 작업 {view.counts.open}개
          </p>
          <p>
            서비스: 실행 중 {view.counts.services}개 · 작업·서비스 전체 {view.counts.active}개
          </p>
        </>
      )}
      {view.countsProblem && <p role="alert">실행 요약을 조회하지 못했습니다. 연결을 확인하고 다시 조회하세요.</p>}
      {view.nativeProblem && <p role="alert">설치 준비 상태를 확인하지 못했습니다. 상태를 확인하고 다시 조회하세요.</p>}
      {view.native?.blocked && <p role="alert">{blockedMessage(view.native)}</p>}
      <Button disabled={view.loading || busy} onClick={retry}>
        다시 조회
      </Button>
    </Dialog>
  );
}
