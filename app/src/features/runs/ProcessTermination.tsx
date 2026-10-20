import { useQueryClient } from "@tanstack/react-query";
import { useRef, useState } from "react";
import { RpcError } from "../../rpc/client";
import { useRpcMutation } from "../../rpc/query";
import { Banner, Button, ConfirmDialog } from "../../ui";
export type ProcessTarget = { pid: number; name: string; startTimeMs: number };
export function useProcessTermination() {
  const qc = useQueryClient();
  const mutation = useRpcMutation("observe.kill");
  const [target, setTarget] = useState<ProcessTarget>();
  const [notice, setNotice] = useState<{ error: boolean; text: string }>();
  const active = useRef(false);
  const [busy, setBusy] = useState(false);
  const refresh = () =>
    Promise.all([
      qc.invalidateQueries({ queryKey: ["observe.ports"] }),
      qc.invalidateQueries({ queryKey: ["observe.processes"] }),
    ]);
  const terminate = async () => {
    if (!target || active.current) return;
    active.current = true;
    setBusy(true);
    const snapshot = target;
    setTarget(undefined);
    setNotice(undefined);
    try {
      await mutation.mutateAsync({ pid: snapshot.pid, signal: "term", expect: { startTimeMs: snapshot.startTimeMs } });
      setNotice({ error: false, text: `PID ${snapshot.pid}에 종료를 요청했습니다.` });
    } catch (error) {
      const code = error instanceof RpcError ? error.code : "internal";
      setNotice({
        error: true,
        text:
          code === "observe.state_changed"
            ? "프로세스가 바뀌었습니다. 갱신된 목록에서 대상을 다시 확인하세요."
            : code === "observe.permission_denied"
              ? "이 프로세스를 종료할 권한이 없습니다. 본인 소유 프로세스만 종료할 수 있습니다."
              : "종료 요청을 확인하지 못했습니다. 목록을 새로 불러온 뒤 다시 확인하세요.",
      });
    } finally {
      await refresh();
      active.current = false;
      setBusy(false);
    }
  };
  return {
    request: (next: ProcessTarget) => {
      if (!active.current) setTarget({ ...next });
    },
    busy,
    feedback: (
      <>
        {notice && <Banner tone={notice.error ? "danger" : "info"}>{notice.text}</Banner>}
        {target && (
          <ConfirmDialog
            open
            title="프로세스 종료"
            confirmLabel="종료"
            body={`${target.name} · PID ${target.pid} · 시작 시각 ${new Date(target.startTimeMs).toLocaleString("ko-KR")} (${target.startTimeMs}ms). 종료를 요청합니다. 저장하지 않은 작업을 잃을 수 있으며 되돌릴 수 없습니다.`}
            onCancel={() => setTarget(undefined)}
            onConfirm={() => void terminate()}
          />
        )}
      </>
    ),
  };
}
export function TerminateButton({
  target,
  busy,
  onRequest,
}: {
  target?: ProcessTarget;
  busy: boolean;
  onRequest: (target: ProcessTarget) => void;
}) {
  return (
    <Button
      size="sm"
      variant="danger"
      disabled={!target || busy}
      aria-label={target ? `PID ${target.pid} 종료` : "프로세스 종료 불가"}
      title={target ? undefined : "종료할 프로세스의 식별 정보를 확인할 수 없습니다."}
      onClick={() => target && onRequest(target)}
    >
      종료
    </Button>
  );
}
