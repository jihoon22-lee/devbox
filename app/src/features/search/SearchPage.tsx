import { useNavigate } from "@tanstack/react-router";
import { useEffect, useRef, useState } from "react";
import type { Scope, SearchQuery } from "../../rpc/gen/rpc";
import { messageFor } from "../../rpc/messages";
import { Button } from "../../ui";
import { CodePreview } from "./CodePreview";
import { SavedSearches } from "./SavedSearches";
import { SearchResults } from "./SearchResults";
import s from "./search.module.css";
import { hitKey } from "./searchChunk";
import { useSearchStream } from "./useSearchStream";

const initial: SearchQuery = {
  text: "",
  scopes: ["notes", "files", "code"],
  projectIds: null,
  regex: false,
  caseSensitive: false,
  limit: 100,
};
export function SearchPage({ onOpenNote }: { onOpenNote(path: string): void }) {
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => input.current?.focus(), []);
  const [draft, setDraft] = useState(initial);
  const [query, setQuery] = useState<SearchQuery>();
  const result = useSearchStream(query);
  const selected = Object.values(result.groups)
    .flat()
    .find((hit) => hitKey(hit) === result.selection);
  const apply = (next: SearchQuery) => {
    setDraft(next);
    setQuery(next);
  };
  return (
    <main className={s.page}>
      <h2>통합 검색</h2>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (draft.text.trim() && draft.scopes.length) {
            if (JSON.stringify(query) === JSON.stringify(draft)) result.retry();
            else setQuery({ ...draft });
          }
        }}
      >
        <label>
          검색어{" "}
          <input
            ref={input}
            name="search-text"
            aria-label="검색어"
            value={draft.text}
            onChange={(event) => setDraft({ ...draft, text: event.target.value })}
          />
        </label>
        <Button type="submit" disabled={!draft.text.trim() || !draft.scopes.length}>
          검색
        </Button>
        <div className={s.options}>
          {(
            [
              ["notes", "노트"],
              ["code", "코드"],
              ["files", "파일 이름"],
            ] as const
          ).map(([scope, name]) => (
            <label key={scope}>
              <input
                type="checkbox"
                checked={draft.scopes.includes(scope)}
                onChange={(event) =>
                  setDraft({
                    ...draft,
                    scopes: event.target.checked
                      ? [...draft.scopes, scope as Scope]
                      : draft.scopes.filter((s) => s !== scope),
                  })
                }
              />
              {name}
            </label>
          ))}
          <label>
            <input
              type="checkbox"
              checked={draft.regex}
              onChange={(event) => setDraft({ ...draft, regex: event.target.checked })}
            />
            정규식
          </label>
          <label>
            <input
              type="checkbox"
              checked={draft.caseSensitive}
              onChange={(event) => setDraft({ ...draft, caseSensitive: event.target.checked })}
            />
            대소문자 구분
          </label>
        </div>
      </form>
      {result.refreshing && <p role="status">검색 중… 이전 결과를 유지합니다.</p>}
      {result.error && (
        <p role="alert">
          {messageFor(result.error.code, result.error.detail).title} <Button onClick={result.retry}>다시 시도</Button>
        </p>
      )}
      {query && !result.refreshing && !result.error && !Object.values(result.groups).some((items) => items.length) && (
        <p>검색 결과가 없습니다.</p>
      )}
      {result.truncated.length > 0 && <p>결과가 많아 일부만 표시했습니다. 검색어를 더 구체적으로 입력하세요.</p>}
      <div className={s.body}>
        <SearchResults
          groups={result.groups}
          selected={result.selection}
          onSelect={(hit) => result.select(hitKey(hit))}
        />
        <div className={s.preview}>
          {selected?.root === "vault" ? (
            <Button onClick={() => onOpenNote(selected.path)}>노트 열기</Button>
          ) : (
            selected && <CodePreview key={hitKey(selected)} hit={selected} />
          )}
          <SavedSearches query={query} onSelect={apply} />
        </div>
      </div>
    </main>
  );
}
export function SearchRoute() {
  const navigate = useNavigate();
  return <SearchPage onOpenNote={(path) => void navigate({ to: "/notes", search: { path } })} />;
}
