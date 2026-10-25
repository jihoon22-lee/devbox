/**
 * 편집기 보내기·취소. 실행기와 같은 변수 규칙(`sendState.ts`), 응답에는 만든 요청의 출처를 붙인다(AR-F08).
 * 진행 중인 송신은 탭 id로 store 모듈에 둔다. 보내는 중 저장해 탭 key가 바뀌거나 다른 탭을 보고 와도
 * 응답이 그 탭에 붙고 [취소]가 같은 송신을 멈춘다.
 * 검토 전 프로젝트 파일·재검토 표시가 있는 파일은 확인 창([검토하고 보내기])을 거쳐 검토를 기록한 뒤에만 보내고,
 * 검토하지 않은 프로젝트 환경의 값은 쓰지 않는다(07b 34행).
 */
import { messageFor } from "../../../rpc/messages";
import { SendFailure, useApiBackend } from "./backend";
import { originOf, prepareSend } from "./sendState";
import { abortSend, beginSend, endSend, isSending, sendReviewNeeded, useApiTabs } from "./store";

export const ENV_REVIEW_BLOCK = "환경 파일을 검토한 뒤 보낼 수 있습니다. 환경의 [검토하고 사용]을 누르세요.";

export function useSend() {
  const backend = useApiBackend();
  async function send(id: string) {
    const { tabs, environment, session, updateById } = useApiTabs.getState();
    const tab = tabs.find((t) => t.id === id);
    if (!tab) return;
    if (isSending(id)) {
      // 버튼은 [취소]로 바뀌어 있지만 Ctrl+Enter는 조용히 무시되지 않게 알린다.
      updateById(id, { notice: "이미 보내는 중입니다. 멈추려면 [취소]를 누르세요." });
      return;
    }
    if (sendReviewNeeded(tab)) {
      updateById(id, { askSendReview: true });
      return;
    }
    const origin = tab.path ? { root: tab.root, path: tab.path } : null;
    const prepared = prepareSend(tab.draft, environment?.variables ?? [], session, origin, tab.root);
    if ("blocked" in prepared) {
      updateById(id, { notice: prepared.blocked });
      return;
    }
    if (environment?.unreviewed && prepared.request.env.variables.some((v) => v.origin === "environment")) {
      updateById(id, { notice: ENV_REVIEW_BLOCK });
      return;
    }
    const controller = beginSend(id);
    if (!controller) return;
    const draft = tab.draft;
    updateById(id, { sending: true, failure: undefined, notice: undefined });
    try {
      const response = await backend.send(prepared.request, controller.signal);
      const name = useApiTabs.getState().tabs.find((t) => t.id === id)?.name ?? tab.name;
      useApiTabs
        .getState()
        .updateById(id, { sending: false, result: { response, origin: originOf(name, draft, response.sentAtMs) } });
    } catch (error) {
      const failure =
        error instanceof SendFailure ? { code: error.code, detail: error.detail } : { code: "internal" as const };
      useApiTabs.getState().updateById(id, { sending: false, failure });
    } finally {
      endSend(id, controller);
    }
  }
  return {
    send,
    cancel: abortSend,
    dismissReview: (id: string) => useApiTabs.getState().updateById(id, { askSendReview: false }),
    /** 사용자가 확인함: 프로젝트 파일은 연 digest로 검토를 기록하고(실패하거나 그사이 파일이 바뀌면 보내지 않음) 보낸다. */
    async confirmReview(id: string) {
      const { tabs, updateById } = useApiTabs.getState();
      const tab = tabs.find((t) => t.id === id);
      if (!tab) return;
      updateById(id, { askSendReview: false });
      try {
        if (tab.root.kind === "project" && tab.path && tab.digest) await backend.review(tab.root, tab.path, tab.digest);
      } catch (error) {
        const code = error instanceof SendFailure ? error.code : "internal";
        const detail = error instanceof SendFailure ? error.detail : undefined;
        updateById(id, { notice: messageFor(code as never, detail)?.title ?? "검토를 기록하지 못했습니다." });
        return;
      }
      const latest = useApiTabs.getState().tabs.find((t) => t.id === id);
      if (!latest) return;
      // 기록하는 동안 [다시 읽기]·저장으로 내용이 바뀌었으면 기록한 digest와 보낼 내용이 다르다.
      if (latest.key !== tab.key || latest.digest !== tab.digest) {
        updateById(id, { notice: "검토하는 동안 파일이 바뀌었습니다. 내용을 확인한 뒤 다시 보내세요." });
        return;
      }
      const parsed = latest.parsed && {
        ...latest.parsed,
        file: { ...latest.parsed.file, requiresSecretReview: false },
      };
      useApiTabs.getState().updateById(id, { reviewed: true, parsed });
      await send(id);
    },
  };
}
