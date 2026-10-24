/** 실시간 연결 메시지 목록(가상 목록): 시각·방향(보냄/받음)·이벤트 이름과 id·내용, 버린·지운 수. */
import { VirtualList } from "../../../../ui/VirtualList";
import s from "../Api.module.css";
import type { Entry, Log } from "./messages";

const MAX_SHOWN = 2_000;
const time = (ms: number | null) => (ms === null ? "" : new Date(ms).toLocaleTimeString());

function line(entry: Entry): string {
  const text = entry.text.length > MAX_SHOWN ? `${entry.text.slice(0, MAX_SHOWN)} …` : entry.text;
  switch (entry.kind) {
    case "message":
      return `${entry.direction === "sent" ? "보냄" : "받음"}${entry.binary ? ` 이진 ${entry.bytes}B` : ""} ${text}`;
    case "event":
      return `${entry.event}${entry.eventId ? ` #${entry.eventId}` : ""} ${text}`;
    case "notice":
      return `— ${text}`;
  }
}

export function MessageList({ log, label }: { log: Log; label: string }) {
  const counts = [
    `${log.entries.filter((e) => e.kind !== "notice").length}개`,
    log.dropped ? `버림 ${log.dropped}개` : "",
    log.trimmed ? `오래되어 지움 ${log.trimmed}개` : "",
  ].filter(Boolean);
  return (
    <div className={s.table}>
      <p className={s.muted} role="status" aria-label={`${label} 개수`}>
        {counts.join(" · ")}
      </p>
      <VirtualList
        label={label}
        items={log.entries}
        follow
        renderItem={(entry) => (
          <div className={s.mono} aria-label={entry.kind === "notice" ? "알림" : undefined}>
            <span className={s.muted}>{time(entry.atMs)} </span>
            {line(entry)}
          </div>
        )}
      />
    </div>
  );
}
