import { useState } from "react";
import type { WebhookReceivedDetail } from "../../../rpc/gen/rpc";
import { Button } from "../../../ui";
import { CodeView } from "../../../ui/code/CodeView";
export function ReceivedDetail({
  detail,
  onReplay,
  onFixture,
}: {
  detail: WebhookReceivedDetail;
  onReplay: () => void;
  onFixture: (name: string) => void;
}) {
  const [name, setName] = useState("");
  const bytes = Uint8Array.from(atob(detail.bodyBase64), (c) => c.charCodeAt(0));
  let text: string;
  try {
    text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    text = `Base64: ${detail.bodyBase64}`;
  }
  const truncated = detail.previewTruncated || detail.received.truncated;
  return (
    <section aria-label="수신 상세">
      <h3>
        {detail.received.method} {detail.received.path}
      </h3>
      <p>{detail.query}</p>
      <dl>
        {detail.headers.map(([key, value], i) => (
          <div key={`${key}-${i}`}>
            <dt>{key}</dt>
            <dd>{value}</dd>
          </div>
        ))}
      </dl>
      <CodeView ariaLabel="수신 본문" value={text} readOnly />
      {truncated && <p role="status">본문이 잘려 다시 보내기와 fixture 저장을 사용할 수 없습니다.</p>}
      <p>API 편집기에서 전송 대상을 확인한 뒤 보내세요. 프로젝트에 저장할 때 인증 정보 등 비밀 값은 가려집니다.</p>
      <Button disabled={truncated} onClick={onReplay}>
        API 요청으로 열기
      </Button>
      <label>
        fixture 이름 <input value={name} onChange={(e) => setName(e.target.value)} maxLength={256} />
      </label>
      <Button disabled={truncated || !name.trim()} onClick={() => onFixture(name)}>
        fixture 저장
      </Button>
    </section>
  );
}
