import type { SearchQuery } from "../../rpc/gen/rpc";

/**
 * The "current project only" box. It drives `projectIds` itself: checked means a project list,
 * unchecked means every project. A saved search can bring another project's id, which the box then names.
 */
export function ProjectScope({
  query,
  currentId,
  nameOf,
  onChange,
}: {
  query: SearchQuery;
  currentId?: string;
  nameOf(id: string): string;
  onChange(projectIds: string[] | null): void;
}) {
  const ids = query.projectIds;
  if (!currentId && !ids) return null;
  const other = ids !== null && (ids.length !== 1 || ids[0] !== currentId);
  const shown = other ? ids.map(nameOf).join(", ") : nameOf(currentId ?? "");
  return (
    <>
      <label>
        <input
          type="checkbox"
          checked={ids !== null}
          onChange={(event) => onChange(event.target.checked ? (currentId ? [currentId] : ids) : null)}
        />
        {other ? "이 프로젝트만" : "현재 프로젝트만"}: {shown}
      </label>
      {ids !== null && query.scopes.includes("notes") && (
        <small>노트는 프로젝트에 속하지 않아 늘 전체 노트에서 찾습니다.</small>
      )}
    </>
  );
}
