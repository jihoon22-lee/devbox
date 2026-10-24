/** 메서드·URL·보내기/취소. Enter·Ctrl+Enter로 보낸다. */
import { Button } from "../../../ui";
import s from "./Api.module.css";

const METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

export function UrlBar({
  method,
  url,
  sending,
  onMethod,
  onUrl,
  onSend,
  onCancel,
}: {
  method: string;
  url: string;
  sending: boolean;
  onMethod: (method: string) => void;
  onUrl: (url: string) => void;
  onSend: () => void;
  onCancel: () => void;
}) {
  return (
    <div className={s.bar}>
      <select
        className={s.select}
        aria-label="메서드"
        value={method}
        onChange={(event) => onMethod(event.target.value)}
      >
        {(METHODS.includes(method) ? METHODS : [method, ...METHODS]).map((m) => (
          <option key={m} value={m}>
            {m}
          </option>
        ))}
      </select>
      <input
        className={`${s.input} ${s.mono} ${s.grow}`}
        aria-label="URL"
        placeholder="https://… 또는 {{baseUrl}}/path"
        value={url}
        onChange={(event) => onUrl(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter" && !event.nativeEvent.isComposing) {
            event.preventDefault();
            onSend();
          }
        }}
      />
      {sending ? (
        <Button onClick={onCancel}>취소</Button>
      ) : (
        <Button variant="primary" onClick={onSend}>
          보내기
        </Button>
      )}
    </div>
  );
}
