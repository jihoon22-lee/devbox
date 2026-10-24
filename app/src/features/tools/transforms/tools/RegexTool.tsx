import { useEffect, useState } from "react";
import { ToolOutput, ToolTextArea, ToolTextField } from "../frame";
import { type RegexResult, runRegex, segments } from "../lib/regex";
export function RegexTool() {
  const [pattern, setPattern] = useState("");
  const [flags, setFlags] = useState("u");
  const [text, setText] = useState("");
  const [result, setResult] = useState<RegexResult>({ matches: [], truncated: false });
  const [error, setError] = useState("");
  const [running, setRunning] = useState(false);
  useEffect(() => {
    const controller = new AbortController();
    setResult({ matches: [], truncated: false });
    setError("");
    if (!pattern) {
      setRunning(false);
      return;
    }
    setRunning(true);
    void runRegex(pattern, flags, text, controller.signal)
      .then((value) => {
        if (!controller.signal.aborted) setResult(value);
      })
      .catch((e) => {
        if (!controller.signal.aborted) setError(e instanceof Error ? e.message : "정규식을 처리하지 못했습니다.");
      })
      .finally(() => {
        if (!controller.signal.aborted) setRunning(false);
      });
    return () => controller.abort();
  }, [pattern, flags, text]);
  return (
    <div className="tool">
      <label>
        정규식
        <ToolTextField aria-label="정규식" value={pattern} onValueChange={setPattern} maxLength={8192} />
      </label>
      <label>
        옵션
        <ToolTextField aria-label="정규식 옵션" value={flags} onValueChange={setFlags} />
      </label>
      <ToolTextArea aria-label="검사할 텍스트" className="io-input" value={text} onValueChange={setText} rows={8} />
      {error && <p role="alert">{error}</p>}
      <p role="status">
        {running ? "검사 중…" : `${result.matches.length}개 일치${result.truncated ? " · 처음 10,000개 표시" : ""}`}
      </p>
      <ToolOutput asDiv ariaLabel="정규식 일치 결과" className="io-output" value={text}>
        {segments(text, result.matches).map((part, index) =>
          part.matched ? (
            <mark key={`${index}-${part.text.length}`}>{part.text || "∅"}</mark>
          ) : (
            <span key={`${index}-${part.text.length}`}>{part.text}</span>
          ),
        )}
      </ToolOutput>
    </div>
  );
}
