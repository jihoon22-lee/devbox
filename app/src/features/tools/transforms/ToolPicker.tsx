import { CATALOG, type ToolId } from "./catalog";
export function ToolPicker({
  selected,
  recent,
  onSelect,
}: {
  selected: ToolId;
  recent: ToolId[];
  onSelect: (id: ToolId) => void;
}) {
  const groups = [...new Set(CATALOG.map((tool) => tool.group))];
  return (
    <div className="tool-picker">
      <label>
        변환 도구{" "}
        <select aria-label="변환 도구" value={selected} onChange={(e) => onSelect(e.target.value as ToolId)}>
          {groups.map((group) => (
            <optgroup key={group} label={group}>
              {CATALOG.filter((tool) => tool.group === group).map((tool) => (
                <option key={tool.id} value={tool.id}>
                  {tool.name}
                </option>
              ))}
            </optgroup>
          ))}
        </select>
      </label>
      <div aria-label="최근 변환 도구">
        {recent.map((id) => {
          const tool = CATALOG.find((t) => t.id === id);
          return tool ? (
            <button type="button" key={id} onClick={() => onSelect(id)}>
              {tool.name}
            </button>
          ) : null;
        })}
      </div>
    </div>
  );
}
