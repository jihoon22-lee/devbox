export default function App() {
  return (
    <main className="app-shell">
      <header className="app-header">
        <div>
          <p className="eyebrow">WORKBENCH</p>
          <h1>Code Pad</h1>
        </div>
        <button type="button" className="open-button" disabled>
          파일 열기
        </button>
      </header>
      <section className="empty-state" aria-label="Code Pad 준비 중">
        <div className="empty-icon" aria-hidden="true">
          &lt;/&gt;
        </div>
        <h2>파일을 열어 시작하세요</h2>
        <p>Code Pad 파일 편집기는 다음 단계에서 연결됩니다.</p>
        <span className="status-pill">파일 I/O 준비 완료</span>
      </section>
    </main>
  );
}
