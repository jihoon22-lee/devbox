import { useRpcQuery } from "../../rpc/query";
export function NotesSettings() {
  const templates = useRpcQuery("notes.templates", {});
  return (
    <section aria-label="노트 설정">
      <h3>노트</h3>
      <p>
        노트는 WSL 보관함의 Markdown 파일로 저장합니다. 기본 위치는 <code>~/notes</code>이며, 다른 폴더는 설정 파일의{" "}
        <code>[notes] vault</code>로 지정할 수 있습니다.
      </p>
      <p>Ctrl+Alt+D로 오늘의 일일 기록을 엽니다. 빠른 캡처의 전역 단축키는 단축키 설정에서 바꿀 수 있습니다.</p>
      <h4>사용할 수 있는 템플릿</h4>
      {templates.data && (
        <ul>
          {templates.data.items.map((item) => (
            <li key={item.path}>{item.name}</li>
          ))}
        </ul>
      )}
      {templates.data?.items.length === 0 && <p>등록된 템플릿이 없습니다.</p>}
      {templates.error && <p role="alert">템플릿 목록을 불러오지 못했습니다.</p>}
    </section>
  );
}
