import { useRpcQuery } from "../../rpc/query";
import { Banner, Button } from "../../ui";
import s from "./notes.module.css";
import { FAILURE_LABEL, INCOMPLETE_LABEL } from "./renameStatus";

/** `/notes?rename=<opId>` (from the notification center): the notes a rename could not fix. */
export function RenameReport({
  opId,
  onOpen,
  onDismiss,
}: {
  opId: string;
  onOpen(path: string): void;
  onDismiss(): void;
}) {
  const status = useRpcQuery("notes.rename_status", { opId });
  const close = (
    <Button size="sm" variant="ghost" onClick={onDismiss}>
      닫기
    </Button>
  );
  if (status.isError)
    return (
      <Banner tone="info" actions={close}>
        이 이름 바꾸기 기록은 더 이상 없습니다(devbox를 다시 시작하면 사라집니다). 다른 노트의 링크를 직접 확인해
        주세요.
      </Banner>
    );
  if (!status.data || status.data.state === "running") return null;
  const { failed, reason, truncated } = status.data;
  return (
    <section className={s.panel} aria-label="링크를 고치지 못한 노트">
      <Banner tone={failed.length || reason ? "warn" : "info"} actions={close}>
        {failed.length
          ? `링크를 고치지 못한 노트 ${failed.length}개${truncated ? " 이상" : ""}`
          : reason
            ? INCOMPLETE_LABEL[reason]
            : "모든 링크를 고쳤습니다."}
      </Banner>
      <ul className={s.failures}>
        {failed.map((item) => (
          <li key={item.path}>
            <button type="button" className={s.linkButton} onClick={() => onOpen(item.path)}>
              {item.path}
            </button>
            <span className={s.count}>{FAILURE_LABEL[item.reason]}</span>
          </li>
        ))}
      </ul>
    </section>
  );
}
