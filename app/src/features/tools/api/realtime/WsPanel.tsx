/** WebSocket 도구: 대상·하위 프로토콜 → [연결 열기] → 메시지 목록·보내기·닫기. 화면을 떠나면 연결을 닫는다. */
import { useState } from "react";
import { Banner, Button } from "../../../../ui";
import s from "../Api.module.css";
import { useApiTabs } from "../store";
import { MessageComposer } from "./MessageComposer";
import { MessageList } from "./MessageList";
import { failureText, type Target, targetVariables, wsParams } from "./messages";
import { TargetFields } from "./TargetFields";
import { useRealtime } from "./useRealtime";

export const STATUS = {
  idle: "연결 전",
  connecting: "연결 중",
  open: "열림",
  reconnecting: "다시 연결 중",
  closed: "닫힘",
  failed: "실패",
} as const;

export const emptyTarget = (): Target => ({
  url: "",
  headers: [],
  tls: { verify: true, caPath: null, clientCertPath: null, clientKeyPath: null },
});

export function WsPanel() {
  const [target, setTarget] = useState(emptyTarget);
  const [subprotocols, setSubprotocols] = useState("");
  const [notice, setNotice] = useState<string>();
  const rt = useRealtime();
  const busy = rt.log.status === "connecting" || rt.log.status === "open";
  const connect = () => {
    const { environment, session } = useApiTabs.getState();
    const resolved = targetVariables(target, environment, session);
    if ("blocked" in resolved) return setNotice(resolved.blocked);
    setNotice(undefined);
    void rt.open({ ws: wsParams(target, subprotocols, resolved.variables) });
  };
  const failure = rt.log.failure && failureText(rt.log.failure);
  return (
    <section className={s.pane} aria-label="WebSocket">
      <div className={s.bar}>
        <TargetFields target={target} placeholder="wss://… 또는 {{wsUrl}}" onChange={setTarget} />
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
          aria-label="하위 프로토콜"
          placeholder="chat, v2 (쉼표로 구분)"
          value={subprotocols}
          onChange={(event) => setSubprotocols(event.target.value)}
        />
        <span role="status" aria-label="연결 상태">
          {STATUS[rt.log.status]}
          {rt.log.protocol ? ` · ${rt.log.protocol}` : ""}
        </span>
      </div>
      {notice && <Banner tone="warn">{notice}</Banner>}
      {failure && <Banner tone="danger">{failure}</Banner>}
      <MessageList log={rt.log} label="WebSocket 메시지" />
      <MessageComposer disabled={rt.log.status !== "open"} onSend={(input) => rt.send(input)} />
    </section>
  );
}
