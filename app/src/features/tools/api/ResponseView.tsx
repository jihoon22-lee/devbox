/** 응답: 출처·상태·시간·크기, 헤더, 본문(JSON은 보기 좋게). 실패는 오류 표의 문구로 보인다. */
import { useMemo, useState } from "react";
import { messageFor } from "../../../rpc/messages";
import { EmptyState, Tabs } from "../../../ui";
import s from "./Api.module.css";
import type { SentResponse } from "./backend";
import type { RequestDraft } from "./model/draft";
import { ResponseOrigin } from "./ResponseOrigin";
import type { ResponseOriginInfo } from "./sendState";

function bodyText(response: SentResponse): string {
  if (response.binary) return `이진 본문 ${response.sizeBytes} 바이트는 표시하지 않습니다.`;
  const text = new TextDecoder().decode(response.body);
  const type = response.headers.find((h) => h.name.toLowerCase() === "content-type")?.value ?? "";
  if (/json/i.test(type) || /^\s*[[{]/.test(text)) {
    try {
      return JSON.stringify(JSON.parse(text), null, 2);
    } catch {
      return text;
    }
  }
  return text;
}

const size = (bytes: number) => (bytes < 1024 ? `${bytes} B` : `${(bytes / 1024).toFixed(1)} KiB`);

export function ResponseView({
  result,
  draft,
  failure,
}: {
  result?: { response: SentResponse; origin: ResponseOriginInfo };
  draft: RequestDraft;
  failure?: { code: string; detail?: unknown };
}) {
  const [view, setView] = useState<"body" | "headers">("body");
  const response = result?.response;
  const text = useMemo(() => (response ? bodyText(response) : ""), [response]);
  if (failure) {
    const message = messageFor(failure.code as never, failure.detail);
    return (
      <div role="alert">
        <strong>{message?.title ?? "요청을 보내지 못했습니다."}</strong>
        {message?.action && <p className={s.muted}>{message.action}</p>}
      </div>
    );
  }
  if (!result || !response) return <EmptyState title="응답 없음" body="요청을 보내면 응답이 여기에 표시됩니다" />;
  return (
    <>
      <ResponseOrigin origin={result.origin} draft={draft} />
      <div className={s.bar}>
        <strong>{`${response.status} ${response.reason}`}</strong>
        <span className={s.muted}>{`${response.elapsedMs} ms`}</span>
        <span className={s.muted}>{size(response.sizeBytes)}</span>
        <span className={s.muted}>{response.httpVersion}</span>
      </div>
      {response.truncated && <p className={s.muted}>본문이 10 MiB를 넘어 앞부분만 보입니다. 전체는 기록에서 보세요.</p>}
      <Tabs
        label="응답 보기"
        items={[
          { id: "body", label: "본문" },
          { id: "headers", label: `헤더 ${response.headers.length}` },
        ]}
        active={view}
        onSelect={(id) => setView(id as "body" | "headers")}
      />
      {view === "body" ? (
        <pre className={s.body}>{text}</pre>
      ) : (
        <table className={s.results} aria-label="응답 헤더">
          <tbody>
            {response.headers.map((header, index) => (
              <tr key={index}>
                <th scope="row">{header.name}</th>
                <td className={s.mono}>{header.value}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </>
  );
}
