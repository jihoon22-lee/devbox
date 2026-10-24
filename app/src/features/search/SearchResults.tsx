import type { SearchHit } from "../../rpc/gen/rpc";
import s from "./search.module.css";
import { hitKey } from "./searchChunk";
import type { SearchGroups } from "./useSearchStream";

const names = { notes: "노트", files: "파일 이름", code: "코드" } as const;
function Snippet({ hit }: { hit: SearchHit }) {
  let at = 0;
  const parts = [];
  for (const [from, to] of hit.rangesUtf16) {
    if (from < at) continue;
    parts.push(hit.snippet.slice(at, from), <mark key={`${from}-${to}`}>{hit.snippet.slice(from, to)}</mark>);
    at = to;
  }
  parts.push(hit.snippet.slice(at));
  return <span>{parts}</span>;
}
export function SearchResults({
  groups,
  selected,
  onSelect,
}: {
  groups: SearchGroups;
  selected?: string;
  onSelect(hit: SearchHit): void;
}) {
  return (
    <div className={s.results}>
      {(["notes", "files", "code"] as const).map(
        (scope) =>
          groups[scope].length > 0 && (
            <section key={scope} aria-label={`${names[scope]} 검색 결과`}>
              <h3>
                {names[scope]} · {groups[scope].length}
              </h3>
              {groups[scope].map((hit) => (
                <button
                  key={hitKey(hit)}
                  type="button"
                  className={s.hit}
                  aria-pressed={selected === hitKey(hit)}
                  onClick={() => onSelect(hit)}
                >
                  <strong>{hit.title || hit.path}</strong>
                  <small>
                    {hit.path}
                    {hit.line ? `:${hit.line}` : ""}
                  </small>
                  <Snippet hit={hit} />
                </button>
              ))}
            </section>
          ),
      )}
    </div>
  );
}
