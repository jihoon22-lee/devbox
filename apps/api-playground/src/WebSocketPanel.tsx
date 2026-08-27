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
  idle: "Idle",
  connecting: "Connecting",
  open: "Open",
  closing: "Closing",
  closed: "Closed",
  error: "Error",
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
        code {message.closeCode ?? "-"}{message.closeReason ? ` — ${message.closeReason}` : ""}
      </span>
    );
  }
  return (
    <span className="websocket-payload websocket-binary-payload">
      <span>{message.binarySize ?? 0} bytes</span>
      {message.binaryHex && <code>hex: {message.binaryHex}</code>}
      {message.binaryText && <code>utf-8: {message.binaryText}</code>}
      {message.kind === "binary" && (
        <button
          type="button"
          className="btn mini"
          onClick={() => onSaveBinary(message.id)}
          disabled={busy}
          aria-label={`Save binary message ${message.id}`}
        >
          Save binary
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
          aria-label="Connect WebSocket"
        >
          Connect
        </button>
        <button
          type="button"
          className="btn danger-outline"
          onClick={onDisconnect}
          disabled={!canDisconnect || busy}
          aria-label="Disconnect WebSocket"
        >
          Disconnect
        </button>
      </div>

      <p className="websocket-hint" id="websocket-transport-hint">
        Native transport supports ws/wss headers, auth, text/binary frames and ping/pong. The browser preview
        is offline-safe but cannot attach custom headers, auth or direct ping/pong.
      </p>

      <div className="websocket-controls">
        <fieldset>
          <legend>Send frame</legend>
          <div className="websocket-field-row">
            <label>
              <span>Type</span>
              <select value={sendKind} onChange={(event) => setSendKind(event.currentTarget.value as "text" | "binary")}>
                <option value="text">Text</option>
                <option value="binary">Binary</option>
              </select>
            </label>
            <label>
              <span>Encoding</span>
              <select
                value={sendEncoding}
                disabled={sendKind === "text"}
                onChange={(event) => setSendEncoding(event.currentTarget.value as PayloadEncoding)}
                aria-describedby="websocket-encoding-hint"
              >
                <option value="text">UTF-8 text</option>
                <option value="hex">Hex</option>
              </select>
            </label>
          </div>
          <label className="websocket-wide-field">
            <span>{sendKind === "binary" && sendEncoding === "hex" ? "Hex payload" : "Payload"}</span>
            <textarea
              rows={3}
              value={sendValue}
              onChange={(event) => setSendValue(event.currentTarget.value)}
              aria-label="WebSocket send payload"
              aria-describedby="websocket-encoding-hint"
              spellCheck={false}
            />
          </label>
          <button
            type="button"
            className="btn"
            onClick={() => onSend(sendKind, sendValue, sendEncoding)}
            disabled={!connected || busy}
            aria-label="Send WebSocket message"
          >
            Send
          </button>
        </fieldset>

        <fieldset>
          <legend>Ping</legend>
          <div className="websocket-field-row">
            <label className="websocket-grow-field">
              <span>Payload</span>
              <input
                value={pingValue}
                onChange={(event) => setPingValue(event.currentTarget.value)}
                aria-label="WebSocket ping payload"
                spellCheck={false}
              />
            </label>
            <label>
              <span>Encoding</span>
              <select value={pingEncoding} onChange={(event) => setPingEncoding(event.currentTarget.value as PayloadEncoding)}>
                <option value="text">UTF-8 text</option>
                <option value="hex">Hex</option>
              </select>
            </label>
          </div>
          <button
            type="button"
            className="btn"
            onClick={() => onPing(pingValue, pingEncoding)}
            disabled={!native || !connected || busy}
            title={native ? "Send a WebSocket ping" : "Ping is available in the native desktop transport"}
          >
            Ping
          </button>
        </fieldset>

        <fieldset>
          <legend>Close</legend>
          <div className="websocket-field-row">
            <label>
              <span>Code</span>
              <input
                type="number"
                min={1000}
                max={4999}
                value={closeCode}
                onChange={(event) => setCloseCode(event.currentTarget.value)}
                aria-label="WebSocket close code"
              />
            </label>
            <label className="websocket-grow-field">
              <span>Reason</span>
              <input
                value={closeReason}
                onChange={(event) => setCloseReason(event.currentTarget.value)}
                aria-label="WebSocket close reason"
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
            Close
          </button>
        </fieldset>
      </div>

      <p className="websocket-hint" id="websocket-encoding-hint">
        Text and binary messages are limited to 4 MiB; ping/pong payloads and close reasons use RFC limits.
        Raw binary stays in memory until you explicitly save it.
      </p>

      <div className="websocket-log-header">
        <span>Messages</span>
        <span className="dim" aria-live="polite">
          {messages.length} retained{dropped > 0 ? ` · ${dropped} evicted` : ""}
        </span>
      </div>
      <ol className="websocket-log" role="log" aria-live="polite" aria-label="WebSocket messages" aria-relevant="additions">
        {messages.map((message) => (
          <li key={`${message.direction}-${message.id}`} className="websocket-message">
            <span className={`websocket-direction websocket-direction-${message.direction}`}>
              {message.direction === "sent" ? "Sent" : "Received"}
            </span>
            <span className="websocket-message-kind">{message.kind}</span>
            <MessageContent message={message} busy={busy} onSaveBinary={onSaveBinary} />
            {message.textTruncated || message.binaryTruncated ? <span className="dim">preview truncated</span> : null}
          </li>
        ))}
        {messages.length === 0 && <li className="websocket-empty">Connect to start a WebSocket session.</li>}
      </ol>
    </section>
  );
}
