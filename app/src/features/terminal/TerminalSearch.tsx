import type { SearchAddon } from "@xterm/addon-search";
import { useEffect, useRef, useState } from "react";
import { Button } from "../../ui";
import s from "./TerminalView.module.css";
export function TerminalSearch({ addon, onClose }: { addon?: SearchAddon; onClose: () => void }) {
  const [text, setText] = useState("");
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    input.current?.focus();
    return () => addon?.clearDecorations();
  }, [addon]);
  return (
    <div className={s.search}>
      <input
        ref={input}
        aria-label="터미널 검색"
        value={text}
        onChange={(event) => {
          setText(event.target.value);
          if (event.target.value) addon?.findNext(event.target.value, { incremental: true });
          else addon?.clearDecorations();
        }}
        onKeyDown={(event) => {
          if (event.nativeEvent.isComposing || event.keyCode === 229) return;
          event.stopPropagation();
          if (event.key === "Enter") {
            event.preventDefault();
            if (text) {
              if (event.shiftKey) addon?.findPrevious(text);
              else addon?.findNext(text);
            }
          }
          if (event.key === "Escape") {
            event.preventDefault();
            onClose();
          }
        }}
      />
      <Button
        size="sm"
        onClick={() => {
          if (text) addon?.findPrevious(text);
        }}
      >
        이전
      </Button>
      <Button
        size="sm"
        onClick={() => {
          if (text) addon?.findNext(text);
        }}
      >
        다음
      </Button>
      <Button size="sm" onClick={onClose}>
        닫기
      </Button>
    </div>
  );
}
