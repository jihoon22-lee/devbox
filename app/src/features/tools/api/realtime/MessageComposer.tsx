/**
 * WebSocket 보내기: 텍스트·JSON·이진(Base64), 닫기(코드·이유), 저장한 메시지(이 화면 안에서만, 파일 저장은 후속).
 * 입력 검사는 데몬과 같다(4 MiB, RFC 6455 닫기 코드).
 */
import { useState } from "react";
import type { WsInput } from "../../../../rpc/gen/rpc";
import { Button } from "../../../../ui";
import s from "../Api.module.css";
import { type ComposeMode, closeInput, compose } from "./messages";

const MODES: { id: ComposeMode; label: string }[] = [
  { id: "text", label: "텍스트" },
  { id: "json", label: "JSON" },
  { id: "binary", label: "이진(Base64)" },
];

export function MessageComposer({ disabled, onSend }: { disabled: boolean; onSend: (input: WsInput) => void }) {
  const [mode, setMode] = useState<ComposeMode>("text");
  const [text, setText] = useState("");
  const [code, setCode] = useState("");
  const [reason, setReason] = useState("");
  const [error, setError] = useState<string>();
  const [saved, setSaved] = useState<{ mode: ComposeMode; text: string }[]>([]);
  const run = (result: { input: WsInput } | { error: string }) => {
    if ("error" in result) return setError(result.error);
    setError(undefined);
    onSend(result.input);
  };
  return (
    <div className={s.table}>
      <div className={s.bar}>
        <select
          className={s.select}
          aria-label="보낼 형식"
          value={mode}
          onChange={(event) => setMode(event.target.value as ComposeMode)}
        >
          {MODES.map((m) => (
            <option key={m.id} value={m.id}>
              {m.label}
            </option>
          ))}
        </select>
        <Button size="sm" variant="primary" disabled={disabled} onClick={() => run(compose(mode, text))}>
          보내기
        </Button>
        <Button size="sm" variant="ghost" disabled={!text} onClick={() => setSaved([...saved, { mode, text }])}>
          메시지 저장
        </Button>
      </div>
      <textarea
        className={`${s.input} ${s.mono}`}
        style={{ height: 96, padding: "var(--space-2)" }}
        aria-label="보낼 메시지"
        value={text}
        onChange={(event) => setText(event.target.value)}
      />
      {error && <p role="alert">{error}</p>}
      {saved.length > 0 && (
        <ul aria-label="저장한 메시지" className={s.tree}>
          {saved.map((item, index) => (
            <li key={`${index}:${item.text.slice(0, 16)}`}>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => {
                  setMode(item.mode);
                  setText(item.text);
                }}
              >
                {`${MODES.find((m) => m.id === item.mode)?.label}: ${item.text.slice(0, 40)}`}
              </Button>
            </li>
          ))}
        </ul>
      )}
      <div className={s.bar}>
        <input
          className={s.input}
          aria-label="닫기 코드"
          placeholder="1000"
          value={code}
          onChange={(event) => setCode(event.target.value)}
        />
        <input
          className={`${s.input} ${s.grow}`}
          aria-label="닫기 이유"
          value={reason}
          onChange={(event) => setReason(event.target.value)}
        />
        <Button size="sm" disabled={disabled} onClick={() => run(closeInput(code, reason))}>
          닫기 보내기
        </Button>
      </div>
    </div>
  );
}
