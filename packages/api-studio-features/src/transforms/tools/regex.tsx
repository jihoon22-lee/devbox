import { useEffect, useState } from "react";
import { regexTest } from "../api";
import type { RegexMatch } from "../types";
import { ToolOutput, ToolTextArea, ToolTextField } from "./common";

/** 정규식 매치를 하이라이트로 표시하는 도구 */
export function RegexTool() {
  const [pattern, setPattern] = useState("");
  const [text, setText] = useState("");
  const [matches, setMatches] = useState<RegexMatch[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!pattern || !text) {
      setMatches([]);
      setError(null);
      return;
    }
    let cancelled = false;
    regexTest(pattern, text)
      .then((ms) => {
        if (cancelled) return;
        setMatches(ms);
        setError(null);
      })
      .catch((e) => {
        if (cancelled) return;
        setMatches([]);
        setError(e instanceof Error ? e.message : String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [pattern, text]);

  const segments: { text: string; matched: boolean }[] = [];
  let cursor = 0;
  for (const m of matches) {
    if (m.start > cursor) segments.push({ text: text.slice(cursor, m.start), matched: false });
    segments.push({ text: text.slice(m.start, m.end), matched: true });
    cursor = m.end;
  }
  if (cursor < text.length) segments.push({ text: text.slice(cursor), matched: false });

  return (
    <div className="tool">
      <ToolTextField
        aria-label="정규식 패턴"
        className="io-input row-input"
        placeholder="정규식 패턴을 입력하세요..."
        value={pattern}
        onValueChange={setPattern}
        spellCheck={false}
      />
      <ToolTextArea
        aria-label="정규식 테스트 입력"
        className="io-input"
        rows={6}
        placeholder="테스트할 텍스트를 입력하세요..."
        value={text}
        onValueChange={setText}
        spellCheck={false}
      />
      <div className="io-label">{matches.length}개 일치</div>
      <ToolOutput
        className="regex-view"
        value={text}
        ariaLabel="정규식 결과"
        downloadName="dev-toolbox-regex-result.txt"
      >
        {segments.length === 0
          ? " "
          : segments.map((s, i) => (
              <span key={i} className={s.matched ? "regex-hit" : undefined}>
                {s.text}
              </span>
            ))}
      </ToolOutput>
      {error && <div className="error-inline">{error}</div>}
    </div>
  );
}
