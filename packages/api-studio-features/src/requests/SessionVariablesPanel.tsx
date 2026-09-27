import { useUndo } from "@devbox/product-shell/undo";
import { useState } from "react";
import type { SessionVariables } from "./lib/runner";
interface Props {
  session: SessionVariables;
  onChange: () => void;
  disabled?: boolean;
}
export function SessionVariablesPanel({ session, onChange, disabled = false }: Props) {
  const { offer, toast } = useUndo();
  const [revealed, setRevealed] = useState(new Map<string, string>());
  const change = () => {
    setRevealed(new Map());
    onChange();
  };
  const discard = (names?: string[]) => {
    const undo = session.discard(names);
    change();
    offer("세션 변수를 지웠습니다.", async () => {
      await undo();
      change();
    });
  };
  return (
    <section aria-label="세션 변수">
      <h3>세션 변수</h3>
      <p>캡처 값은 앱을 닫으면 사라집니다.</p>
      {session.entries().map((item) => (
        <div key={item.name}>
          <span>{item.name}</span> <span>{revealed.get(item.name) === item.plain ? item.plain : "••••••••"}</span>
          <button
            type="button"
            aria-label={`${item.name} ${revealed.get(item.name) === item.plain ? "숨기기" : "보기"}`}
            onClick={() =>
              setRevealed((previous) => {
                const next = new Map(previous);
                if (next.get(item.name) === item.plain) next.delete(item.name);
                else next.set(item.name, item.plain);
                return next;
              })
            }
          >
            {revealed.get(item.name) === item.plain ? "숨기기" : "보기"}
          </button>
          <button
            type="button"
            disabled={disabled}
            aria-label={`${item.name} 지우기`}
            onClick={() => {
              discard([item.name]);
            }}
          >
            지우기
          </button>
        </div>
      ))}
      <button
        type="button"
        disabled={disabled || !session.entries().length}
        onClick={() => {
          discard();
        }}
      >
        모두 지우기
      </button>
      {toast}
    </section>
  );
}
