import { useState } from "react";
import type {
  WebSocketConnectionState,
  WebSocketMessage,
} from "./types";

type PayloadEncoding = "text" | "hex";

export interface WebSocketPanelProps {
  state: WebSocketConnectionState;
  messages: readonly WebSocketMessage[];
  dropped: number;
  native: boolean;
  canConnect: boolean;
  busy: boolean;
  onConnect: () => void;
  onDisconnect: () => void;
  onSend: (kind: "text" | "binary", value: string, encoding: PayloadEncoding) => void;
  onPing: (value: string, encoding: PayloadEncoding) => void;
  onClose: (code: number | undefined, reason: string) => void;
  onSaveBinary: (messageId: number) => void;
}

const STATE_LABELS: Record<WebSocketConnectionState, string> = {
  idle: "대기",
  connecting: "연결 중",
  open: "열림",
  closing: "닫는 중",
  closed: "닫힘",
  error: "오류",
};

function MessageContent({
  message,
  busy,
  onSaveBinary,
}: {
  message: WebSocketMessage;
  busy: boolean;
  onSaveBinary: (messageId: number) => void;
}) {
  if (message.kind === "text") {
    return <code className="websocket-payload">{message.text ?? ""}</code>;
  }
  if (message.kind === "close") {
    return (
      <span className="websocket-payload">
        코드 {message.closeCode ?? "-"}{message.closeReason ? ` — ${message.closeReason}` : ""}
      </span>
    );
  }
  return (
    <span className="websocket-payload websocket-binary-payload">
      <span>{message.binarySize ?? 0}바이트</span>
      {message.binaryHex && <code>hex: {message.binaryHex}</code>}
      {message.binaryText && <code>utf-8: {message.binaryText}</code>}
      {message.kind === "binary" && (
        <button
          type="button"
          className="btn mini"
          onClick={() => onSaveBinary(message.id)}
          disabled={busy}
          aria-label={`Binary 메시지 ${message.id} 저장`}
        >
          Binary 저장
        </button>
      )}
    </span>
  );
}

export function WebSocketPanel({
  state,
  messages,
  dropped,
  native,
  canConnect,
  busy,
  onConnect,
  onDisconnect,
  onSend,
  onPing,
  onClose,
  onSaveBinary,
}: WebSocketPanelProps) {
  const [sendKind, setSendKind] = useState<"text" | "binary">("text");
  const [sendEncoding, setSendEncoding] = useState<PayloadEncoding>("text");
  const [sendValue, setSendValue] = useState("");
  const [pingEncoding, setPingEncoding] = useState<PayloadEncoding>("text");
  const [pingValue, setPingValue] = useState("");
  const [closeCode, setCloseCode] = useState("1000");
  const [closeReason, setCloseReason] = useState("");
  const connected = state === "open";
  const connecting = state === "connecting";
  const canDisconnect = connected || connecting;

  return (
    <section className="websocket-panel" aria-labelledby="websocket-heading" aria-busy={busy}>
      <div className="websocket-header">
        <h2 id="websocket-heading">WebSocket</h2>
        <span
          className={`websocket-state websocket-state-${state}`}
          role="status"
          aria-live="polite"
          aria-label={`WebSocket ${STATE_LABELS[state]}`}
        >
          {STATE_LABELS[state]}
        </span>
        <span className="spacer" />
        <button
          type="button"
          className="btn"
          onClick={onConnect}
          disabled={!canConnect || busy || connected || connecting}
          aria-label="WebSocket 연결"
        >
          연결
        </button>
        <button
          type="button"
          className="btn danger-outline"
          onClick={onDisconnect}
          disabled={!canDisconnect || busy}
          aria-label="WebSocket 연결 해제"
        >
          연결 해제
        </button>
      </div>

      <p className="websocket-hint" id="websocket-transport-hint">
        네이티브 전송은 ws/wss 헤더, auth, 텍스트/바이너리 프레임과 ping/pong을 지원합니다. 브라우저 미리보기는
        오프라인에서도 안전하지만 사용자 지정 헤더·auth·직접 ping/pong은 연결할 수 없습니다.
      </p>

      <div className="websocket-controls">
        <fieldset>
          <legend>프레임 전송</legend>
          <div className="websocket-field-row">
            <label>
              <span>형식</span>
              <select value={sendKind} onChange={(event) => setSendKind(event.currentTarget.value as "text" | "binary")}>
                <option value="text">텍스트</option>
                <option value="binary">바이너리</option>
              </select>
            </label>
            <label>
              <span>인코딩</span>
              <select
                value={sendEncoding}
                disabled={sendKind === "text"}
                onChange={(event) => setSendEncoding(event.currentTarget.value as PayloadEncoding)}
                aria-describedby="websocket-encoding-hint"
              >
                <option value="text">UTF-8 텍스트</option>
                <option value="hex">Hex</option>
              </select>
            </label>
          </div>
          <label className="websocket-wide-field">
            <span>{sendKind === "binary" && sendEncoding === "hex" ? "Hex 페이로드" : "페이로드"}</span>
            <textarea
              rows={3}
              value={sendValue}
              onChange={(event) => setSendValue(event.currentTarget.value)}
              aria-label="WebSocket 전송 페이로드"
              aria-describedby="websocket-encoding-hint"
              spellCheck={false}
            />
          </label>
          <button
            type="button"
            className="btn"
            onClick={() => onSend(sendKind, sendValue, sendEncoding)}
            disabled={!connected || busy}
            aria-label="WebSocket 메시지 보내기"
          >
            보내기
          </button>
        </fieldset>

        <fieldset>
          <legend>Ping</legend>
          <div className="websocket-field-row">
            <label className="websocket-grow-field">
              <span>페이로드</span>
              <input
                value={pingValue}
                onChange={(event) => setPingValue(event.currentTarget.value)}
                aria-label="WebSocket ping 페이로드"
                spellCheck={false}
              />
            </label>
            <label>
              <span>인코딩</span>
              <select value={pingEncoding} onChange={(event) => setPingEncoding(event.currentTarget.value as PayloadEncoding)}>
                <option value="text">UTF-8 텍스트</option>
                <option value="hex">Hex</option>
              </select>
            </label>
          </div>
          <button
            type="button"
            className="btn"
            onClick={() => onPing(pingValue, pingEncoding)}
            disabled={!native || !connected || busy}
            title={native ? "WebSocket ping 보내기" : "Ping은 네이티브 데스크톱 전송에서 사용할 수 있습니다"}
          >
            Ping
          </button>
        </fieldset>

        <fieldset>
          <legend>연결 닫기</legend>
          <div className="websocket-field-row">
            <label>
              <span>코드</span>
              <input
                type="number"
                min={1000}
                max={4999}
                value={closeCode}
                onChange={(event) => setCloseCode(event.currentTarget.value)}
                aria-label="WebSocket 연결 종료 코드"
              />
            </label>
            <label className="websocket-grow-field">
              <span>사유</span>
              <input
                value={closeReason}
                onChange={(event) => setCloseReason(event.currentTarget.value)}
                aria-label="WebSocket 연결 종료 사유"
                spellCheck={false}
              />
            </label>
          </div>
          <button
            type="button"
            className="btn"
            onClick={() => {
              const parsed = closeCode.trim() ? Number(closeCode) : undefined;
              onClose(Number.isFinite(parsed) ? parsed : undefined, closeReason);
            }}
            disabled={!connected || busy}
          >
            닫기
          </button>
        </fieldset>
      </div>

      <p className="websocket-hint" id="websocket-encoding-hint">
        텍스트와 바이너리 메시지는 4MiB로 제한되며 ping/pong 페이로드와 연결 종료 사유에는 RFC 제한이 적용됩니다.
        바이너리 원문은 명시적으로 저장할 때까지 메모리에만 보관됩니다.
      </p>

      <div className="websocket-log-header">
        <span>메시지</span>
        <span className="dim" aria-live="polite">
          {messages.length}개 유지{dropped > 0 ? ` · ${dropped}개 제외됨` : ""}
        </span>
      </div>
      <div className="websocket-log" role="log" aria-live="polite" aria-label="WebSocket 메시지" aria-relevant="additions">
        {messages.map((message) => (
          <div key={`${message.direction}-${message.id}`} className="websocket-message">
            <span className={`websocket-direction websocket-direction-${message.direction}`}>
              {message.direction === "sent" ? "보냄" : "받음"}
            </span>
            <span className="websocket-message-kind">{message.kind}</span>
            <MessageContent message={message} busy={busy} onSaveBinary={onSaveBinary} />
            {message.textTruncated || message.binaryTruncated ? <span className="dim">미리보기 일부 생략</span> : null}
          </div>
        ))}
        {messages.length === 0 && <div className="websocket-empty">WebSocket 세션을 시작하려면 연결하세요.</div>}
      </div>
    </section>
  );
}
