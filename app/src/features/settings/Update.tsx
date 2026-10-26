import { Button } from "../../ui";
import { type UpdateIssue, type UpdateSnapshot, useNativeUpdate } from "./nativeUpdate";
import { openSettings } from "./open";

const messages: Record<UpdateIssue, string> = {
  network: "업데이트 서버에 연결하지 못했습니다. 인터넷 연결을 확인한 뒤 다시 시도하세요.",
  invalidRelease: "업데이트 정보를 확인하지 못했습니다. 잠시 후 다시 시도하세요.",
  size: "업데이트 파일 크기가 올바르지 않습니다. 다시 내려받으세요.",
  digest: "업데이트 파일 검증에 실패했습니다. 다시 내려받으세요.",
  timeout: "업데이트 요청 시간이 초과됐습니다. 연결을 확인한 뒤 다시 시도하세요.",
  storage: "업데이트 파일을 저장하지 못했습니다. 디스크 공간과 쓰기 권한을 확인한 뒤 다시 시도하세요.",
  cancelled: "업데이트가 취소되었습니다. 필요하면 다시 시도하세요.",
  staleOperation: "업데이트 작업이 바뀌었습니다. 현재 상태를 다시 불러오세요.",
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
      return "업데이트를 적용하고 있습니다.";
    case "failed":
      return "업데이트를 완료하지 못했습니다.";
  }
}

export function Update() {
  const control = useNativeUpdate();
  const { snapshot } = control;
  const issue = control.issue ?? snapshot?.error;
  const reload = issue === "unavailable" || issue === "invalidSnapshot" || issue === "staleOperation";
  return (
    <section aria-label="앱 업데이트">
      <h3>앱 업데이트</h3>
      {!control.native ? (
        <p>앱 업데이트는 Windows 앱에서 확인할 수 있습니다.</p>
      ) : (
        <>
          {snapshot && <p>앱 {snapshot.currentVersion}</p>}
          {snapshot?.enabled === false ? (
            <p>
              {snapshot.disabledReason === "channel"
                ? "이 채널에서는 공개 업데이트를 사용할 수 없습니다. 정식 Windows 앱에서 확인하세요."
                : "개발 버전에서는 공개 업데이트를 사용할 수 없습니다. 정식 버전에서 확인하세요."}
            </p>
          ) : snapshot ? (
            <>
              {snapshot.availableVersion && <p>새 버전 {snapshot.availableVersion}</p>}
              <p role="status">{phaseLabel(snapshot)}</p>
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
              {snapshot.nextAction !== "none" && (
                <Button
                  disabled={control.busy || !control.connected || reload}
                  onClick={() => {
                    if (snapshot.nextAction !== "none") void control.run(snapshot.nextAction);
                  }}
                >
                  {actions[snapshot.nextAction]}
                </Button>
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
        : snapshot?.enabled && snapshot.phase !== "idle"
          ? phaseLabel(snapshot)
          : `앱 ${snapshot?.currentVersion}`}
    </button>
  );
}
