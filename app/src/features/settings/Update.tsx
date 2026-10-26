import { useState } from "react";
import { Button } from "../../ui";
import { type UpdateIssue, type UpdateSnapshot, useNativeUpdate } from "./nativeUpdate";
import { openSettings } from "./open";
import { UpdateConfirmation } from "./UpdateConfirmation";

const messages: Record<UpdateIssue, string> = {
  network: "업데이트 서버에 연결하지 못했습니다. 인터넷 연결을 확인한 뒤 다시 시도하세요.",
  invalidRelease: "업데이트 정보를 확인하지 못했습니다. 잠시 후 다시 시도하세요.",
  size: "업데이트 파일 크기가 올바르지 않습니다. 다시 내려받으세요.",
  digest: "업데이트 파일 검증에 실패했습니다. 다시 내려받으세요.",
  timeout: "업데이트 요청 시간이 초과됐습니다. 연결을 확인한 뒤 다시 시도하세요.",
  storage: "업데이트 파일을 저장하지 못했습니다. 디스크 공간과 쓰기 권한을 확인한 뒤 다시 시도하세요.",
  cancelled: "업데이트가 취소되었습니다. 필요하면 다시 시도하세요.",
  staleOperation: "업데이트 작업이 바뀌었습니다. 현재 상태를 다시 불러오세요.",
  nativeActive: "앱에서 실행한 Windows 작업 때문에 설치할 수 없습니다. 작업이 끝나거나 직접 중지한 뒤 다시 시도하세요.",
  nativeUnknown: "Windows 작업 상태를 확인하지 못해 설치할 수 없습니다. 실행 상태를 확인한 뒤 다시 시도하세요.",
  quitRejected: "열린 창의 저장 준비가 끝나지 않아 설치를 취소했습니다. 문서 상태를 확인하고 다시 시도하세요.",
  runner: "업데이트 설치 준비에 실패했습니다. 앱 상태를 확인한 뒤 다시 시도하세요.",
  installFailed: "업데이트 설치에 실패했습니다. 현재 앱 버전과 업데이트 정보를 다시 확인하세요.",
  relaunchFailed: "업데이트 뒤 앱을 다시 열지 못했습니다. 시작 메뉴에서 Devbox를 열어 설치 상태를 확인하세요.",
  recoveryRequired: "업데이트 설치 상태를 확인할 수 없습니다. 설치 프로그램이 끝났는지 확인한 뒤 앱을 다시 시작하세요.",
  unavailable: "업데이트 상태를 확인하지 못했습니다. 상태를 다시 불러오세요.",
  invalidSnapshot: "업데이트 상태가 올바르지 않습니다. 상태를 다시 불러오세요.",
};
const actions = { check: "새 버전 확인", download: "업데이트 다운로드", cancel: "다운로드 취소" } as const;

function phaseLabel(snapshot: UpdateSnapshot) {
  switch (snapshot.phase) {
    case "idle":
      return "새 버전을 확인할 수 있습니다.";
    case "checking":
      return "새 버전을 확인하고 있습니다.";
    case "available":
      return "새 업데이트를 내려받을 수 있습니다.";
    case "downloading":
      return "업데이트를 내려받고 있습니다.";
    case "ready":
      return "업데이트 파일 준비 완료";
    case "installing":
      if (snapshot.error === "recoveryRequired") return messages.recoveryRequired;
      if (snapshot.error === "runner")
        return "업데이트 설치 도우미 정리를 확인하고 있습니다. 확인이 끝날 때까지 기다리세요.";
      if (snapshot.error === "cancelled") return "설치 준비를 취소하고 있습니다.";
      return snapshot.nextAction === "cancel" ? "설치 준비 중입니다." : "앱 종료 후 업데이트를 설치합니다.";
    case "failed":
      return "업데이트를 완료하지 못했습니다.";
  }
}

export function Update() {
  const control = useNativeUpdate();
  const { snapshot } = control;
  const [confirmation, setConfirmation] = useState<number | null>(null);
  const preparing = snapshot?.phase === "installing";
  const issue =
    control.issue ??
    (preparing &&
    (snapshot.error === "cancelled" || snapshot.error === "runner" || snapshot.error === "recoveryRequired")
      ? null
      : snapshot?.error);
  const reload = issue === "unavailable" || issue === "invalidSnapshot" || issue === "staleOperation";
  return (
    <section aria-label="앱 업데이트">
      <h3>앱 업데이트</h3>
      {!control.native ? (
        <p>앱 업데이트는 Windows 앱에서 확인할 수 있습니다.</p>
      ) : (
        <>
          {snapshot && <p>앱 {snapshot.currentVersion}</p>}
          {snapshot?.phase === "installing" && <p role="status">{phaseLabel(snapshot)}</p>}
          {snapshot?.enabled === false ? (
            <p>
              {snapshot.disabledReason === "channel"
                ? "이 채널에서는 공개 업데이트를 사용할 수 없습니다. 정식 Windows 앱에서 확인하세요."
                : "개발 버전에서는 공개 업데이트를 사용할 수 없습니다. 정식 버전에서 확인하세요."}
            </p>
          ) : snapshot ? (
            <>
              {snapshot.availableVersion && <p>새 버전 {snapshot.availableVersion}</p>}
              {snapshot.phase !== "installing" && <p role="status">{phaseLabel(snapshot)}</p>}
              {snapshot.phase === "downloading" && (
                <>
                  <progress
                    aria-label="업데이트 다운로드 진행"
                    max={snapshot.totalBytes || undefined}
                    value={snapshot.totalBytes ? snapshot.receivedBytes : undefined}
                  />
                  <p>
                    {snapshot.receivedBytes.toLocaleString()}바이트
                    {snapshot.totalBytes !== null && ` / ${snapshot.totalBytes.toLocaleString()}바이트`}
                  </p>
                </>
              )}
              {snapshot.nextAction === "install" ? (
                <Button
                  disabled={control.busy || !control.connected || reload}
                  onClick={() => setConfirmation(snapshot.generation)}
                >
                  업데이트
                </Button>
              ) : (
                snapshot.nextAction !== "none" && (
                  <Button
                    disabled={control.busy || !control.connected || reload}
                    onClick={() => {
                      if (snapshot.nextAction !== "none" && snapshot.nextAction !== "install")
                        void control.run(snapshot.nextAction);
                    }}
                  >
                    {snapshot.nextAction === "cancel" && preparing ? "설치 준비 취소" : actions[snapshot.nextAction]}
                  </Button>
                )
              )}
            </>
          ) : (
            !issue && <p role="status">업데이트 상태를 불러오고 있습니다.</p>
          )}
          {issue && <p role="alert">{messages[issue]}</p>}
          {reload && (
            <Button disabled={control.busy} onClick={control.refresh}>
              상태 다시 불러오기
            </Button>
          )}
          {confirmation !== null &&
            snapshot?.enabled &&
            snapshot.phase === "ready" &&
            snapshot.nextAction === "install" &&
            snapshot.generation === confirmation &&
            snapshot.availableVersion && (
              <UpdateConfirmation
                generation={confirmation}
                version={snapshot.availableVersion}
                busy={control.busy}
                onConfirm={control.confirmInstall}
                onCancel={() => setConfirmation(null)}
              />
            )}
        </>
      )}
    </section>
  );
}

export function UpdateStatus() {
  const { native, snapshot, issue } = useNativeUpdate();
  if (!native || (!snapshot && !issue)) return null;
  return (
    <button
      type="button"
      aria-label="앱 업데이트 정보"
      onClick={() => openSettings("about")}
      style={{ background: "transparent", border: 0, cursor: "pointer", color: "inherit" }}
    >
      {issue
        ? "업데이트 조회 실패"
        : snapshot?.phase === "installing" || (snapshot?.enabled && snapshot.phase !== "idle")
          ? snapshot.error === "recoveryRequired"
            ? "업데이트 설치 상태 확인 필요"
            : snapshot.error === "runner"
              ? "업데이트 준비 상태 확인 중"
              : phaseLabel(snapshot)
          : `앱 ${snapshot?.currentVersion}`}
    </button>
  );
}
