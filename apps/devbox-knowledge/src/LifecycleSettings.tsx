export default function LifecycleSettings() {
  return (
    <section className="panel" aria-labelledby="knowledge-close-title">
      <h2 id="knowledge-close-title">창 닫기와 활동 수집</h2>
      <p className="dim">
        설치본은 창을 닫아도 이미 동의한 활동 수집과 검색 색인이 계속됩니다. 활동 화면이나 Devbox 알림 영역 메뉴에서
        수집을 켜거나 일시중지할 수 있습니다.
      </p>
      <p className="dim">
        portable은 창을 닫으면 수집도 중지합니다. 저장하지 않은 노트는 기존 저장·버리기·취소 확인을 거친 뒤 창을
        닫습니다.
      </p>
      <p className="dim">
        제품 창을 닫는 것과 백그라운드 작업 전체 종료는 별개입니다. 모든 작업을 멈추려면 Devbox 알림 영역 메뉴에서
        종료하세요.
      </p>
    </section>
  );
}
