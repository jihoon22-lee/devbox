/**
 * 저장(`expect: digest`)과 새 요청으로 저장(`expect: absent`). 순서:
 * ① 이 PC에서 검토하지 않은 프로젝트 파일이면 첫 저장 전에 검토 확인(저장하면 그 digest로 검토를 기록하므로)
 * ② 정화로 새로 가릴 값이 있으면 [비밀로 옮기기]·[가리고 저장]·[취소]
 * ③ 쓰기, 프로젝트 루트는 쓴 digest로 검토 기록. 충돌이면 [다시 읽기]를 보인다.
 */
import { useRef, useState } from "react";
import type { CollectionExpect } from "../../../rpc/gen/rpc";
import { messageFor } from "../../../rpc/messages";
import { SendFailure, useApiBackend } from "./backend";
import type { RequestDraft } from "./model/draft";
import { fileFromDraft } from "./model/fileDraft";
import { requestFilePath, serializeRequestFile } from "./model/requestFile";
import { applyMoves, MoveSecretsError, moveToSecrets, type SecretMove } from "./moveSecrets";
import { type ApiTab, useApiTabs } from "./store";
import { bumpCollections } from "./useCollections";

interface Target {
  path: string;
  name: string;
  expect: CollectionExpect;
}
export interface PendingMask {
  target: Target;
  masked: string[];
}

const failureText = (error: unknown) => {
  const code = error instanceof SendFailure ? error.code : "internal";
  return (
    messageFor(code as never, error instanceof SendFailure ? error.detail : undefined)?.title ?? "저장하지 못했습니다."
  );
};

export function useSave(tab: ApiTab | undefined) {
  const backend = useApiBackend();
  const [pending, setPending] = useState<PendingMask | null>(null);
  const [pendingReview, setPendingReview] = useState<Target | null>(null);
  const [askName, setAskName] = useState(false);
  const [busy, setBusy] = useState(false);
  // 같은 탭의 저장이 겹치지 않게(Ctrl+S 연타) 동기로 막는다. 화면 상태 `busy`는 버튼 표시용이다.
  const writing = useRef(false);

  const current = () => useApiTabs.getState().tabs.find((t) => t.id === tab?.id);
  const serialize = (t: ApiTab, draft: RequestDraft, name: string) =>
    serializeRequestFile(fileFromDraft(draft, name), t.root, t.parsed?.raw);
  const notify = (t: ApiTab, error: unknown) =>
    useApiTabs.getState().updateById(t.id, {
      notice: failureText(error),
      conflict: error instanceof SendFailure && error.code === "api.collection_conflict" && !!t.path,
    });

  /** 쓰는 초안을 지금 고정한다. 응답이 온 뒤의 store 초안이 아니라 이 초안으로 `saved`를 계산한다. */
  /** 거짓이면 쓰지 않았다(탭이 없거나 다른 저장이 진행 중). */
  async function write(target: Target, draft?: RequestDraft): Promise<boolean> {
    const t = current();
    if (!t || writing.current) return false;
    writing.current = true;
    if (draft) useApiTabs.getState().setDraftById(t.id, draft);
    const snapshot = draft ?? t.draft;
    const { text } = serialize(t, snapshot, target.name);
    setBusy(true);
    try {
      const written = await backend.write(t.root, target.path, text, target.expect);
      if (t.root.kind === "project") await backend.review(t.root, target.path, written.digest);
      useApiTabs.getState().markSaved(t.id, { ...target, digest: written.digest, text, draft: snapshot });
      bumpCollections();
    } catch (error) {
      notify(t, error);
    } finally {
      writing.current = false;
      setBusy(false);
    }
    return true;
  }

  /** 옮긴 참조를 지금 초안에 반영하고 알린다(옮기는 동안 고친 내용은 덮지 않음). */
  function keepEdits(t: ApiTab, moves: SecretMove[], notice: string) {
    const latest = current();
    if (!latest) return;
    useApiTabs.getState().setDraftById(t.id, applyMoves(latest.draft, moves));
    const names = moves.map((move) => move.reference.slice(9, -2));
    useApiTabs
      .getState()
      .updateById(t.id, { notice: `${notice}${names.length ? ` 옮긴 비밀: ${names.join(", ")}` : ""}` });
  }

  function checkMasks(target: Target) {
    const t = current();
    if (!t) return;
    const { masked } = serialize(t, t.draft, target.name);
    if (masked.length) setPending({ target, masked });
    else void write(target);
  }

  function start(target: Target) {
    const t = current();
    if (!t) return;
    if (!t.reviewed) setPendingReview(target);
    else checkMasks(target);
  }

  return {
    busy,
    pending,
    pendingReview: pendingReview !== null,
    askName,
    save() {
      const t = current();
      if (!t) return;
      if (!t.path || !t.digest) setAskName(true);
      else start({ path: t.path, name: t.name, expect: { kind: "digest", digest: t.digest } });
    },
    saveAsNew: () => setAskName(true),
    cancelName: () => setAskName(false),
    async confirmName(name: string) {
      const t = current();
      if (!t) return;
      setAskName(false);
      const listed = await backend.list(t.root).catch(() => ({ items: [] }));
      const used = new Set(listed.items.map((item) => item.path.toLowerCase()));
      const folder = t.path?.includes("/") ? t.path.slice(0, t.path.lastIndexOf("/")) : "";
      start({ path: requestFilePath(name, folder, used), name, expect: { kind: "absent" } });
    },
    cancelReview: () => setPendingReview(null),
    confirmReview() {
      const target = pendingReview;
      setPendingReview(null);
      if (target) checkMasks(target);
    },
    async reload() {
      const t = current();
      if (!t?.path) return;
      try {
        const file = await backend.read(t.root, t.path);
        // 읽는 동안 다른 이름으로 저장됐으면 그 파일이 아니므로 바꾸지 않는다.
        if (current()?.path === t.path) useApiTabs.getState().reload(t.id, file);
      } catch (error) {
        notify(t, error);
      }
    },
    cancelMask: () => setPending(null),
    maskAndSave() {
      if (!pending) return;
      setPending(null);
      void write(pending.target);
    },
    async moveAndSave() {
      const t = current();
      if (!pending || !t) return;
      setPending(null);
      const before = t.draft;
      try {
        const moved = await moveToSecrets(backend, before, pending.target.name, pending.masked);
        // 옮기는 동안 고쳤거나 다른 이름으로 저장됐거나 다른 저장이 진행 중이면 쓰지 않는다.
        // 어느 경우든 만든 비밀은 참조로 반영해 평문이 초안에 남거나 비밀이 고아가 되지 않게 한다.
        const latest = current();
        const blocked = "다른 저장이 진행 중이라 저장하지 않았습니다. 끝난 뒤 다시 저장해 주세요.";
        if (latest?.key !== t.key)
          keepEdits(t, moved.moves, "옮기는 동안 다른 파일로 저장되어 이 내용은 저장하지 않았습니다.");
        else if (latest.draft !== before)
          keepEdits(t, moved.moves, "옮기는 동안 내용이 바뀌어 저장하지 않았습니다. 확인한 뒤 다시 저장해 주세요.");
        else if (!(await write(pending.target, moved.draft))) keepEdits(t, moved.moves, blocked);
      } catch (error) {
        if (error instanceof MoveSecretsError)
          // 이미 만든 비밀은 그 참조로 바꿔 두고, 파일은 저장하지 않는다.
          keepEdits(
            t,
            error.moves,
            `비밀로 옮기다 멈췄습니다. 파일은 저장하지 않았습니다.${
              error.created.length ? ` 이미 만든 비밀: ${error.created.join(", ")}` : ""
            }`,
          );
        else notify(t, error);
      }
    },
  };
}
