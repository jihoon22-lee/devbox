import type { DocKey } from "../../rpc/gen/rpc";
import { messageFor } from "../../rpc/messages";
import { Banner, Button } from "../../ui";
import { keyId } from "../documents/offlineJournal";
import { useDocuments } from "../documents/store";

const REASONS = {
  leased_elsewhere: "다른 창에서 편집 중입니다.",
  binary: "텍스트가 아닌 파일이라 읽기 전용으로 엽니다.",
  encoding: "UTF-8이 아닌 파일이라 읽기 전용으로 엽니다.",
  too_large: "노트가 2 MiB를 넘어 읽기 전용으로 엽니다.",
} as const;

/** Read-only notes say why; one held by another window can be taken over ([여기로 가져오기] = `documents.take`). */
export function ReadOnlyBar({ docKey }: { docKey: DocKey }) {
  const doc = useDocuments((st) => Object.values(st.docs).find((d) => keyId(d.key) === keyId(docKey)));
  if (!doc || doc.state !== "ReadOnly") return null;
  const reason = doc.readOnly?.reason;
  const takeable = reason === "leased_elsewhere";
  return (
    <Banner
      tone="info"
      actions={
        takeable && (
          <Button size="sm" onClick={() => useDocuments.getState().take(docKey)}>
            여기로 가져오기
          </Button>
        )
      }
    >
      {reason === "too_large" && doc.policy.kind === "code"
        ? "파일이 5 MiB를 넘어 읽기 전용으로 엽니다."
        : reason
          ? REASONS[reason]
          : "읽기 전용으로 열었습니다."}
    </Banner>
  );
}

/** A claim that failed (outside the vault, missing scope, …) leaves the note loading; say why and offer a retry. */
export function OpenError({ docKey }: { docKey: DocKey }) {
  const doc = useDocuments((st) => Object.values(st.docs).find((d) => keyId(d.key) === keyId(docKey)));
  if (!doc || doc.state !== "Loading" || !doc.error) return null;
  const message = messageFor(doc.error);
  const retry = () => {
    const docs = useDocuments.getState();
    docs.close(docKey);
    docs.open(docKey, doc.policy);
  };
  return (
    <Banner
      tone="danger"
      actions={
        <Button size="sm" onClick={retry}>
          다시 시도
        </Button>
      }
    >
      {message.title}
    </Banner>
  );
}
