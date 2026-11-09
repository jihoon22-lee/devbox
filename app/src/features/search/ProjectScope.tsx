import { useId } from "react";
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
  const hint = useId();
  const ids = query.projectIds;
  if (!currentId && !ids) return null;
  const other = ids !== null && (ids.length !== 1 || ids[0] !== currentId);
  const shown = other ? ids.map(nameOf).join(", ") : nameOf(currentId ?? "");
  const notes = ids !== null && query.scopes.includes("notes");
  // An unchecked box is only shown with a current project, so checking always narrows to it.
  return (
    <>
      <label>
        <input
          type="checkbox"
          checked={ids !== null}
          aria-describedby={notes ? hint : undefined}
          onChange={(event) => onChange(event.target.checked && currentId ? [currentId] : null)}
        />
        {other ? "이 프로젝트만" : "현재 프로젝트만"}: {shown}
      </label>
      {notes && <small id={hint}>노트는 프로젝트에 속하지 않아 늘 전체 노트에서 찾습니다.</small>}
    </>
  );
}
