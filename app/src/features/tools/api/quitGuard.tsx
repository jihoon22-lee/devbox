/**
 * 앱 종료 전 확인. 요청 탭과 환경 초안은 화면 메모리에만 있어 앱을 끄면 사라지므로, main 창의 종료 준비
 * (트레이 [종료]·업데이트 [설치하고 다시 시작])가 문서 확인보다 먼저 묻는다. 저장 버튼은 두지 않는다
 * (새 요청은 경로가 없을 수 있고, 초안에 비밀이 있을 수 있어 디스크에 자동 저장하지 않는다).
 * 버리기를 골라도 store는 지우지 않는다. 이어지는 문서 확인에서 취소하면 초안이 그대로 남아야 한다.
 */
import { create } from "zustand";
import { ConfirmDialog } from "../../../ui";
import { dirtyEnvDraftCount } from "./EnvEditor";
import { isDirty, useApiTabs } from "./store";

interface Changes {
  requests: number;
  environments: number;
}
interface Request extends Changes {
  answer(discard: boolean): void;
}
export const useQuitGuard = create<{ request?: Request }>(() => ({}));

/**
 * 저장하지 않은 API 변경이 없으면 묻지 않는다(`undefined`, 문서 확인이 같은 차례에 바로 이어진다).
 * 있으면 [변경 버리고 종료]만 true이고 [취소]·데몬 쪽 종료 취소(`signal`)는 false다.
 */
export function confirmApiQuit(signal: AbortSignal): Promise<boolean> | undefined {
  const changes: Changes = {
    requests: useApiTabs.getState().tabs.filter(isDirty).length,
    environments: dirtyEnvDraftCount(),
  };
  if (!changes.requests && !changes.environments) return undefined;
  return new Promise((resolve) => {
    if (signal.aborted || useQuitGuard.getState().request) return resolve(false);
    let done = false;
    const answer = (discard: boolean) => {
      if (done) return;
      done = true;
      signal.removeEventListener("abort", abort);
      useQuitGuard.setState({ request: undefined });
      resolve(discard);
    };
    const abort = () => answer(false);
    signal.addEventListener("abort", abort, { once: true });
    useQuitGuard.setState({ request: { ...changes, answer } });
  });
}

/** 개수가 0인 종류는 뺀다. */
function body({ requests, environments }: Changes) {
  const kinds: string[] = [];
  if (requests) kinds.push(`요청 ${requests}개`);
  if (environments) kinds.push(`환경 ${environments}개`);
  return `${kinds.join("와 ")}의 변경을 저장하지 않았습니다. 종료하면 이 변경은 사라집니다.`;
}

/** main 창의 다른 종료 확인(`DocumentCloseDialog`)과 같이 연결 화면 밖에 둔다. */
export function ApiQuitDialog() {
  const request = useQuitGuard((state) => state.request);
  return (
    <ConfirmDialog
      open={!!request}
      title="저장하지 않은 API 변경이 있습니다"
      body={request ? body(request) : ""}
      confirmLabel="변경 버리고 종료"
      onCancel={() => request?.answer(false)}
      onConfirm={() => request?.answer(true)}
    />
  );
}
