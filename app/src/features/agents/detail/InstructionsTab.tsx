import { memo } from "react";
import { Button, useToasts } from "../../../ui";
import type { AgentTask } from "../model";
import s from "./Detail.module.css";

type Delivery = NonNullable<AgentTask["followUps"][number]["delivery"]>;
/** Only a delivered instruction says 전달됨. One recorded before delivery was tracked has no state: it is unconfirmed. */
const DELIVERY: Record<Delivery, string> = {
  delivered: "전달됨",
  pending: "보내는 중",
  unknown: "전달 확인 안 됨",
  // Only a first instruction devbox sends itself (Antigravity): held until the tool shows its input box, or never
  // sent because the tool got another instruction first or ended.
  queued: "도구가 입력창을 보이면 보냅니다",
  notSent: "보내지 않음",
};

/** A first instruction's delivery, with why it was not sent where the task's own record does not show it. */
function firstDelivery(first: NonNullable<AgentTask["firstInstruction"]>) {
  const label = DELIVERY[first.delivery];
  return first.reason === "unobserved"
    ? `${label}: Devbox가 꺼져 있던 동안의 입력을 알 수 없어 보내지 않았습니다. 필요하면 직접 보내세요.`
    : label;
}

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
  // Whitespace alone reads as nothing, so it counts as empty; any other text is shown and copied as stored.
  const blank = !text.trim();
  return (
    <li>
      <div className={s.entryHead}>
        <strong>{label}</strong>
        {/* The same local date and time as the header's closing time. */}
        <time dateTime={new Date(atMs).toISOString()}>{new Date(atMs).toLocaleString("ko")}</time>
        {delivery && <span>{delivery}</span>}
        <Button size="sm" disabled={blank} onClick={() => copy(text)}>
          복사
        </Button>
      </div>
      {blank ? empty && <p className={s.entryNote}>{empty}</p> : <p className={s.entryText}>{text}</p>}
    </li>
  );
}

/**
 * What the task was asked: its first prompt, then each follow-up in the order it was sent, with its delivery. The
 * first prompt has a delivery only where devbox sends it itself instead of handing it to the tool as an argument.
 * Only reads the task, so a closed task shows it too; nothing here sends. The detail re-renders every second for its
 * clock while the task object stays the same, so the tab renders again only when the task does.
 */
export const InstructionsTab = memo(function InstructionsTab({ task }: { task: AgentTask }) {
  // Follow-ups sent in the same millisecond keep the order they were stored in.
  const followUps = task.followUps
    .map((followUp, index) => ({ followUp, index }))
    .sort((a, b) => a.followUp.atMs - b.followUp.atMs || a.index - b.index);
  return (
    <div className={s.instructions}>
      <ol aria-label="지시 기록">
        <Entry
          label="처음 지시"
          atMs={task.createdMs}
          text={task.prompt}
          delivery={task.firstInstruction && firstDelivery(task.firstInstruction)}
          empty="지시문 없이 시작했습니다."
        />
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
});
