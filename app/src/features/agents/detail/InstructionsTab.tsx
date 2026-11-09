import { Button, useToasts } from "../../../ui";
import type { AgentTask } from "../model";
import s from "./Detail.module.css";

type Delivery = NonNullable<AgentTask["followUps"][number]["delivery"]>;
/** Only a delivered instruction says 전달됨. One recorded before delivery was tracked has no state: it is unconfirmed. */
const DELIVERY: Record<Delivery, string> = {
  delivered: "전달됨",
  pending: "보내는 중",
  unknown: "전달 확인 안 됨",
};

function copy(text: string) {
  void Promise.resolve()
    .then(() => navigator.clipboard.writeText(text))
    .then(
      () => useToasts.getState().push({ tone: "ok", title: "지시를 복사했습니다" }),
      () => useToasts.getState().push({ tone: "warn", title: "클립보드에 복사하지 못했습니다" }),
    );
}

function Entry({
  label,
  atMs,
  text,
  delivery,
  empty,
}: {
  label: string;
  atMs: number;
  text: string;
  delivery?: string;
  /** Shown in place of an empty text. */
  empty?: string;
}) {
  return (
    <li>
      <div className={s.entryHead}>
        <strong>{label}</strong>
        {/* The same local date and time as the header's closing time. */}
        <time dateTime={new Date(atMs).toISOString()}>{new Date(atMs).toLocaleString("ko")}</time>
        {delivery && <span>{delivery}</span>}
        <Button size="sm" disabled={!text} onClick={() => copy(text)}>
          복사
        </Button>
      </div>
      {text ? <p className={s.entryText}>{text}</p> : empty && <p className={s.entryNote}>{empty}</p>}
    </li>
  );
}

/**
 * What the task was asked: its first prompt, then each follow-up in the order it was sent, with its delivery.
 * Only reads the task, so a closed task shows it too; nothing here sends.
 */
export function InstructionsTab({ task }: { task: AgentTask }) {
  // Follow-ups sent in the same millisecond keep the order they were stored in.
  const followUps = task.followUps
    .map((followUp, index) => ({ followUp, index }))
    .sort((a, b) => a.followUp.atMs - b.followUp.atMs || a.index - b.index);
  return (
    <div className={s.instructions}>
      <ol aria-label="지시 기록">
        <Entry label="처음 지시" atMs={task.createdMs} text={task.prompt} empty="지시문 없이 시작했습니다." />
        {followUps.map(({ followUp, index }) => (
          <Entry
            key={index}
            label="이어서 보낸 지시"
            atMs={followUp.atMs}
            text={followUp.text}
            delivery={DELIVERY[followUp.delivery ?? "unknown"]}
          />
        ))}
      </ol>
      {followUps.length === 0 && <p className={s.entryNote}>이어서 보낸 지시가 없습니다.</p>}
    </div>
  );
}
