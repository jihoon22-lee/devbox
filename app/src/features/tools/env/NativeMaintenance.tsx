import { invoke } from "@tauri-apps/api/core";
import { useRef, useState } from "react";
import { Button, ConfirmDialog, useToasts } from "../../../ui";

type Action = "redeploy_wsl" | "cleanup_wsl";
const ACTIONS = {
  redeploy_wsl: { title: "다시 설정", body: "내장 WSL 실행 파일을 다시 배치하고 이 채널의 데몬을 다시 시작합니다." },
  cleanup_wsl: {
    title: "WSL 쪽 정리",
    body: "이 채널의 터미널 세션을 종료하고 WSL 서비스와 실행 파일을 삭제합니다. 데이터·설정과 다른 채널은 보존합니다. 정리 후 WSL 연결은 멈춥니다.",
  },
};
export function NativeMaintenance() {
  const [action, setAction] = useState<Action | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const running = useRef(false);
  const execute = async () => {
    if (!action || running.current) return;
    running.current = true;
    setBusy(true);
    setAction(null);
    setNotice("");
    try {
      await invoke(action);
      if (action === "cleanup_wsl")
        useToasts.getState().push({ tone: "ok", title: "WSL 쪽 정리가 완료되었습니다. 데이터와 설정은 보존했습니다." });
      setNotice(
        action === "cleanup_wsl"
          ? "WSL 쪽 정리가 완료되었습니다. 데이터와 설정은 남아 있습니다."
          : "WSL 다시 설정을 완료했습니다. 다시 연결되면 점검을 새로고침해 주세요.",
      );
    } catch {
      setNotice("작업을 완료하지 못했습니다. 연결 상태와 설치 환경을 확인해 주세요.");
    } finally {
      running.current = false;
      setBusy(false);
    }
  };
  const native = "__TAURI_INTERNALS__" in window;
  return (
    <section aria-label="WSL 설치 관리">
      <h3>WSL 설치 관리</h3>
      <Button disabled={busy || !native} onClick={() => setAction("redeploy_wsl")}>
        다시 설정
      </Button>{" "}
      <Button disabled={busy || !native} onClick={() => setAction("cleanup_wsl")}>
        WSL 쪽 정리
      </Button>
      {!native && <p>Windows 앱에서 설치를 관리할 수 있습니다.</p>}
      {notice && <p role="status">{notice}</p>}
      <ConfirmDialog
        open={action !== null}
        title={action ? ACTIONS[action].title : "설치 관리"}
        body={action ? ACTIONS[action].body : ""}
        confirmLabel="실행"
        disabled={busy}
        onConfirm={() => void execute()}
        onCancel={() => setAction(null)}
      />
    </section>
  );
}
