import { useUndo } from "@devbox/product-shell/undo";
import { useEffect, useRef, useState } from "react";
import type { SessionVariables } from "./lib/runner";
interface Props {
  session: SessionVariables;
  onChange: () => void;
  disabled?: boolean;
}
export function SessionVariablesPanel({ session, onChange, disabled = false }: Props) {
  const { offer, toast } = useUndo();
  const [revealed, setRevealed] = useState<{ version: number; values: Map<string, string> }>({
    version: session.version,
    values: new Map(),
  });
  const [error, setError] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const epoch = useRef(0);
  const version = session.version;
  useEffect(() => {
    epoch.current++;
    setRevealed({ version, values: new Map() });
    setBusy(null);
    setError("");
    return () => {
      epoch.current++;
    };
  }, [version]);
  const change = () => {
    epoch.current++;
    setRevealed({ version: session.version, values: new Map() });
    setBusy(null);
    setError("");
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
  const toggle = async (name: string) => {
    const token = ++epoch.current,
      revision = session.version;
    const values = new Map(revealed.version === revision ? revealed.values : []);
    if (values.has(name)) {
      values.delete(name);
      setRevealed({ version: revision, values });
      setBusy(null);
      return;
    }
    setError("");
    setBusy(name);
    try {
      const plain = await session.reveal(name);
      if (epoch.current !== token || session.version !== revision) return;
      values.set(name, plain);
      setRevealed({ version: revision, values });
    } catch {
      if (epoch.current === token && session.version === revision)
        setError("캡처 값을 표시하지 못했습니다. 만료되었거나 지워졌을 수 있습니다.");
    } finally {
      if (epoch.current === token) setBusy(null);
    }
  };
  return (
    <section aria-label="세션 변수">
      <h3>세션 변수</h3>
      <p>캡처 값은 앱을 닫으면 사라집니다.</p>
      {session.entries().map((item) => {
        const shown = revealed.version === version && revealed.values.has(item.name);
        return (
          <div key={item.name}>
            <span>{item.name}</span> <span>{shown ? revealed.values.get(item.name) : "••••••••"}</span>
            <button
              type="button"
              disabled={busy === item.name}
              aria-label={`${item.name} ${shown ? "숨기기" : "보기"}`}
              onClick={() => {
                void toggle(item.name);
              }}
            >
              {shown ? "숨기기" : busy === item.name ? "불러오는 중…" : "보기"}
            </button>
            <button
              type="button"
              disabled={disabled}
              aria-label={`${item.name} 지우기`}
              onClick={() => discard([item.name])}
            >
              지우기
            </button>
          </div>
        );
      })}
      <button type="button" disabled={disabled || !session.entries().length} onClick={() => discard()}>
        모두 지우기
      </button>
      {error && <p role="alert">{error}</p>}
      {toast}
    </section>
  );
}
