import { useEffect, useId, useState } from "react";
import { Dialog, Kbd } from "../ui";
import s from "./CommandPalette.module.css";
import { useCommands } from "./commands";
export function CommandPalette({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange} title="명령" width={560} contentId="devbox-command-palette">
      {open && <PaletteContents onClose={() => onOpenChange(false)} />}
    </Dialog>
  );
}
function PaletteContents({ onClose }: { onClose: () => void }) {
  const items = useCommands((state) => state.items);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const id = useId();
  const needle = query.trim().toLocaleLowerCase();
  const list = [...items.values()].filter(
    (c) => c.label.toLocaleLowerCase().includes(needle) || c.id.toLowerCase().includes(needle),
  );
  const selected = Math.min(active, Math.max(0, list.length - 1));
  const activeId = list.length ? `${id}-${selected}` : undefined;
  useEffect(() => {
    if (activeId) document.getElementById(activeId)?.scrollIntoView?.({ block: "nearest" });
  }, [activeId]);
  function run(index: number) {
    const command = list[index];
    if (!command || useCommands.getState().items.get(command.id) !== command) return;
    onClose();
    command.run();
  }
  return (
    <>
      <input
        role="combobox"
        aria-expanded="true"
        aria-controls={id}
        aria-activedescendant={activeId}
        aria-autocomplete="list"
        aria-label="명령 검색"
        value={query}
        placeholder="명령 이름을 입력하세요"
        className={s.input}
        onChange={(event) => {
          setQuery(event.target.value);
          setActive(0);
        }}
        onKeyDown={(event) => {
          if (event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229) return;
          if (event.key === "ArrowDown" || event.key === "ArrowUp") {
            event.preventDefault();
            setActive(Math.max(0, Math.min(list.length - 1, selected + (event.key === "ArrowDown" ? 1 : -1))));
          }
          if (event.key === "Enter") {
            event.preventDefault();
            run(selected);
          }
        }}
      />
      <div id={id} role="listbox" aria-label="명령" className={s.list}>
        {list.map((command, index) => (
          <div
            id={`${id}-${index}`}
            key={command.id}
            role="option"
            aria-selected={index === selected}
            tabIndex={-1}
            className={s.option}
            onMouseEnter={() => setActive(index)}
            onClick={() => run(index)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                run(index);
              }
            }}
          >
            <span>{command.label}</span>
            {command.keys && <Kbd keys={command.keys} />}
          </div>
        ))}
      </div>
      {!list.length && <p role="status">일치하는 명령이 없습니다</p>}
    </>
  );
}
