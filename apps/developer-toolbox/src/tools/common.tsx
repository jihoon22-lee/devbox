import { useEffect, useRef, useState } from "react";

/** async 변환 결과를 입력 변경 시 자동 계산하는 훅 */
export function useAsyncTransform(
  input: string,
  run: (input: string) => Promise<{ output: string; error?: string }>,
) {
  const [output, setOutput] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const seq = useRef(0);

  useEffect(() => {
    const current = ++seq.current;
    setRunning(true);
    run(input)
      .then((res) => {
        if (seq.current !== current) return;
        setOutput(res.output);
        setError(res.error ?? null);
      })
      .catch((e) => {
        if (seq.current !== current) return;
        setError(e instanceof Error ? e.message : String(e));
      })
      .finally(() => {
        if (seq.current === current) setRunning(false);
      });
  }, [input, run]);

  return { output, error, running };
}

/** 입력/출력 2분할 + 복사 버튼을 갖춘 범용 변환 도구 */
export function TransformerTool({
  placeholder,
  run,
  extra,
  rows = 8,
}: {
  placeholder: string;
  run: (input: string) => Promise<{ output: string; error?: string }>;
  extra?: React.ReactNode;
  rows?: number;
}) {
  const [input, setInput] = useState("");
  const { output, error, running } = useAsyncTransform(input, run);

  return (
    <div className="tool">
      {extra}
      <div className="io-grid">
        <div className="io-col">
          <div className="io-label">Input</div>
          <textarea
            className="io-input"
            placeholder={placeholder}
            rows={rows}
            value={input}
            onChange={(e) => setInput(e.currentTarget.value)}
            spellCheck={false}
          />
        </div>
        <div className="io-col">
          <div className="io-label">
            Output {running && <span className="dim">(running...)</span>}
            {output && !error && (
              <button
                className="copy-btn"
                onClick={() => navigator.clipboard.writeText(output)}
              >
                Copy
              </button>
            )}
          </div>
          <pre className={`io-output ${error ? "io-error" : ""}`}>{error || output || " "}</pre>
        </div>
      </div>
    </div>
  );
}

export function CopyBtn({ value }: { value: string }) {
  if (!value) return null;
  return (
    <button className="copy-btn" onClick={() => navigator.clipboard.writeText(value)}>
      Copy
    </button>
  );
}
