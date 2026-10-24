/** SSE 도구: 대상·Last-Event-ID → [연결 열기] → 이벤트 목록(재연결 표시). 화면을 떠나면 연결을 닫는다. */
import { useState } from "react";
import { Banner, Button } from "../../../../ui";
import s from "../Api.module.css";
import { useApiTabs } from "../store";
import { MessageList } from "./MessageList";
import { failureText, sseParams, targetVariables } from "./messages";
import { TargetFields } from "./TargetFields";
import { useRealtime } from "./useRealtime";
import { emptyTarget, STATUS } from "./WsPanel";

export function SsePanel() {
  const [target, setTarget] = useState(emptyTarget);
  const [lastEventId, setLastEventId] = useState("");
  const [notice, setNotice] = useState<string>();
  const rt = useRealtime();
  const busy = rt.log.status === "connecting" || rt.log.status === "open" || rt.log.status === "reconnecting";
  const connect = () => {
    const { environment, session } = useApiTabs.getState();
    const resolved = targetVariables(target, environment, session);
    if ("blocked" in resolved) return setNotice(resolved.blocked);
    setNotice(undefined);
    void rt.open({ sse: sseParams(target, lastEventId, resolved.variables) });
  };
  const failure = rt.log.failure && failureText(rt.log.failure);
  return (
    <section className={s.pane} aria-label="SSE">
      <div className={s.bar}>
        <TargetFields target={target} placeholder="https://…/events 또는 {{baseUrl}}/events" onChange={setTarget} />
        {busy ? (
          <Button onClick={rt.close}>연결 닫기</Button>
        ) : (
          <Button variant="primary" disabled={!target.url.trim()} onClick={connect}>
            연결 열기
          </Button>
        )}
      </div>
      <div className={s.bar}>
        <input
          className={`${s.input} ${s.grow}`}
          aria-label="Last-Event-ID"
          placeholder="처음 연결에 보낼 id(비우면 없음)"
          value={lastEventId}
          onChange={(event) => setLastEventId(event.target.value)}
        />
        <span role="status" aria-label="연결 상태">
          {STATUS[rt.log.status]}
        </span>
        <Button size="sm" variant="ghost" onClick={rt.clear}>
          목록 지우기
        </Button>
      </div>
      {notice && <Banner tone="warn">{notice}</Banner>}
      {failure && <Banner tone="danger">{failure}</Banner>}
      <MessageList log={rt.log} label="SSE 이벤트" />
    </section>
  );
}
