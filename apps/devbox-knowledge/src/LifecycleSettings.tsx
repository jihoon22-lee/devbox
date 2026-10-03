import { collectorMessage, useCollectorStatus } from "./collectorStatus";
export default function LifecycleSettings() {
  const status = useCollectorStatus(true);
  return (
    <section className="panel" aria-labelledby="knowledge-close-title">
      <h2 id="knowledge-close-title">창 닫기와 활동 수집</h2>
      <p className="dim">{collectorMessage(status)}</p>
      <p className="dim">저장하지 않은 노트는 저장·복구본 유지·완전히 버리기·취소 확인을 거친 뒤 창을 닫습니다.</p>
    </section>
  );
}
