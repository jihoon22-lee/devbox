/**
 * 편집기 보내기·취소. 실행기와 같은 변수 규칙(`sendState.ts`), 응답에는 만든 요청의 출처를 붙인다(AR-F08).
 * 진행 중인 송신은 탭 id로 store 모듈에 둔다. 보내는 중 저장해 탭 key가 바뀌거나 다른 탭을 보고 와도
 * 응답이 그 탭에 붙고 [취소]가 같은 송신을 멈춘다.
 */
import { SendFailure, useApiBackend } from "./backend";
import { originOf, prepareSend } from "./sendState";
import { abortSend, beginSend, endSend, isSending, useApiTabs } from "./store";

export function useSend() {
  const backend = useApiBackend();
  return {
    async send(id: string) {
      const { tabs, environment, session, updateById } = useApiTabs.getState();
      const tab = tabs.find((t) => t.id === id);
      if (!tab) return;
      if (isSending(id)) {
        // 버튼은 [취소]로 바뀌어 있지만 Ctrl+Enter는 조용히 무시되지 않게 알린다.
        updateById(id, { notice: "이미 보내는 중입니다. 멈추려면 [취소]를 누르세요." });
        return;
      }
      const origin = tab.path ? { root: tab.root, path: tab.path } : null;
      const prepared = prepareSend(tab.draft, environment?.variables ?? [], session, origin);
      if ("blocked" in prepared) {
        updateById(id, { notice: prepared.blocked });
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
    },
    cancel: abortSend,
  };
}
