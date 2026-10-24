/** 응답을 만든 요청(AR-F08): 이름·메서드·URL·보낸 시각, 지금 편집 중인 요청과 다르면 표시. */
import s from "./Api.module.css";
import type { RequestDraft } from "./model/draft";
import { editedSince, type ResponseOriginInfo } from "./sendState";

export function ResponseOrigin({ origin, draft }: { origin: ResponseOriginInfo; draft: RequestDraft }) {
  const sent = new Date(origin.sentAtMs);
  return (
    <div role="group" aria-label="응답을 만든 요청" className={s.origin}>
      <strong>{origin.name}</strong>
      <span className={s.mono}>{origin.method}</span>
      <span className={s.mono}>{origin.url}</span>
      <time dateTime={sent.toISOString()}>{sent.toLocaleString()}</time>
      {editedSince(origin, draft) && <span className={s.edited}>편집 중인 요청과 다름</span>}
    </div>
  );
}
