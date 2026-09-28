import { useId, useMemo, useState } from "react";
import { generateCode, type CodeTarget } from "./lib/codegen";
import type { RequestTemplate } from "./types";
import type { EnvVariable } from "./lib/environments";
const targets: [CodeTarget, string][] = [
  ["curl", "curl"],
  ["fetch", "JavaScript fetch"],
  ["python", "Python"],
  ["go", "Go"],
  ["csharp", "C#"],
];
export function CodePanel({ request, environment }: { request: RequestTemplate; environment: EnvVariable[] }) {
  const id = useId();
  const [target, setTarget] = useState<CodeTarget>("curl");
  const [copying, setCopying] = useState(false);
  const [copiedCode, setCopiedCode] = useState<string | null>(null);
  const [failedCode, setFailedCode] = useState<string | null>(null);
  const result = useMemo(() => {
    try {
      if (!request.url.trim()) return null;
      return generateCode(target, request, environment);
    } catch {
      return null;
    }
  }, [target, request, environment]);
  const code = result?.code ?? "";
  const copy = async () => {
    if (!code || copying) return;
    setCopying(true);
    setFailedCode(null);
    setCopiedCode(null);
    try {
      await navigator.clipboard.writeText(code);
      setCopiedCode(code);
    } catch {
      setFailedCode(code);
    } finally {
      setCopying(false);
    }
  };
  return (
    <section className="curl-panel" aria-label="요청 코드">
      <h3>코드</h3>
      <div className="tabs" role="tablist" aria-label="코드 언어">
        {targets.map(([value, label], index) => (
          <button
            key={value}
            type="button"
            role="tab"
            id={`${id}-${value}`}
            className={`tab ${value === target ? "active" : ""}`}
            aria-selected={value === target}
            aria-controls={`${id}-panel`}
            tabIndex={value === target ? 0 : -1}
            onClick={() => setTarget(value)}
            onKeyDown={(event) => {
              const next =
                event.key === "ArrowRight"
                  ? (index + 1) % targets.length
                  : event.key === "ArrowLeft"
                    ? (index + targets.length - 1) % targets.length
                    : event.key === "Home"
                      ? 0
                      : event.key === "End"
                        ? targets.length - 1
                        : null;
              if (next === null) return;
              event.preventDefault();
              setTarget(targets[next][0]);
              document.getElementById(`${id}-${targets[next][0]}`)?.focus();
            }}
          >
            {label}
          </button>
        ))}
      </div>
      <div role="tabpanel" id={`${id}-panel`} aria-labelledby={`${id}-${target}`} tabIndex={0}>
        {result ? (
          <>
            {result.placeholders.length > 0 && <p>채워 넣을 값: {result.placeholders.join(", ")}</p>}
            <pre className="curl-text">
              <code>{code}</code>
            </pre>
          </>
        ) : (
          <p role="alert">요청 구성을 확인해 주세요.</p>
        )}
      </div>
      <button type="button" className="copy-btn" disabled={!code || copying} onClick={() => void copy()}>
        복사
      </button>
      {copiedCode === code && code && <p role="status">복사했습니다.</p>}
      {failedCode === code && code && <p role="alert">코드를 복사하지 못했습니다.</p>}
    </section>
  );
}
