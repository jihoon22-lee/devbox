import { type ReactNode, useState } from "react";
import { useTransformInput } from "../handoff";
import { CopyBtn } from "./ToolActions";
import { ToolTextArea } from "./ToolInput";
import { ToolOutput } from "./ToolOutput";
import { useAsyncTransform } from "./useToolAction";
export function TransformerTool({
  placeholder,
  run,
  extra,
  rows = 8,
  clearOutputOnInput = false,
}: {
  placeholder: string;
  run: (input: string, signal?: AbortSignal) => Promise<{ output: string; error?: string }>;
  extra?: ReactNode;
  rows?: number;
  clearOutputOnInput?: boolean;
}) {
  const [input, setInput] = useState(useTransformInput()?.text ?? "");
  const { output, error, running } = useAsyncTransform(input, run, { clearOutputOnStart: clearOutputOnInput });
  return (
    <div className="tool">
      {extra}
      <div className="io-grid">
        <div className="io-col">
          <div className="io-label">입력</div>
          <ToolTextArea
            aria-label="입력"
            aria-busy={running}
            className="io-input"
            placeholder={placeholder}
            rows={rows}
            value={input}
            onValueChange={setInput}
            spellCheck={false}
          />
        </div>
        <div className="io-col">
          <div className="io-label">
            출력 {running && <span role="status">(실행 중...)</span>}
            {output && !error && <CopyBtn value={output} />}
          </div>
          <ToolOutput className={`io-output ${error ? "io-error" : ""}`} value={error || output} />
        </div>
      </div>
    </div>
  );
}
