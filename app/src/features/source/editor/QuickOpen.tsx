import { useEffect, useMemo, useState } from "react";
import type { SearchQuery } from "../../../rpc/gen/rpc";
import { messageFor } from "../../../rpc/messages";
import { Dialog } from "../../../ui";
import { keyId } from "../../documents/offlineJournal";
import { useSearchStream } from "../../search/useSearchStream";
import s from "../source.module.css";
import { useSource } from "../store";
import { rankFiles } from "./quickOpen";
export function QuickOpen({ projectId, onClose }: { projectId: string; onClose(): void }) {
  const [text, setText] = useState("");
  const [query, setQuery] = useState<SearchQuery>();
  const [selected, setSelected] = useState(0);
  const recent = useSource((s) => s.recent);
  useEffect(() => {
    if (!text.trim()) {
      setQuery(undefined);
      return;
    }
    const timer = setTimeout(
      () =>
        setQuery({ text, scopes: ["files"], projectIds: [projectId], regex: false, caseSensitive: false, limit: 300 }),
      120,
    );
    return () => clearTimeout(timer);
  }, [text, projectId]);
  const result = useSearchStream(query);
  const files = useMemo(
    () => rankFiles(text, result.groups.files, recent, `project:${projectId}`),
    [text, result.groups.files, recent, projectId],
  );
  const choose = (index: number) => {
    const file = files[index];
    if (!file) return;
    void useSource.getState().open(file);
    onClose();
  };
  return (
    <Dialog open title="빠른 열기" onOpenChange={(open) => !open && onClose()}>
      <input
        className={s.quickInput}
        aria-label="파일 이름"
        aria-controls="source-quick-files"
        aria-activedescendant={files[selected] ? `source-quick-${selected}` : undefined}
        value={text}
        onChange={(e) => {
          setText(e.currentTarget.value);
          setSelected(0);
        }}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown") {
            e.preventDefault();
            setSelected((i) => Math.min(i + 1, files.length - 1));
          }
          if (e.key === "ArrowUp") {
            e.preventDefault();
            setSelected((i) => Math.max(0, i - 1));
          }
          if (e.key === "Enter") {
            e.preventDefault();
            choose(Math.max(0, Math.min(selected, files.length - 1)));
          }
        }}
      />
      {result.refreshing && <p role="status">파일 찾는 중…</p>}
      {result.error && <p role="alert">{messageFor(result.error.code, result.error.detail).title}</p>}
      <ul id="source-quick-files" className={s.results} role="listbox" aria-label="파일 검색 결과">
        {files.map((file, index) => (
          <li key={keyId(file)}>
            <button
              id={`source-quick-${index}`}
              type="button"
              role="option"
              aria-selected={index === selected}
              onClick={() => choose(index)}
            >
              {file.path}
            </button>
          </li>
        ))}
      </ul>
      {!files.length && !result.refreshing && (
        <p>{text ? "일치하는 파일이 없습니다." : "최근에 연 파일이 없습니다. 파일 이름을 입력하세요."}</p>
      )}
    </Dialog>
  );
}
