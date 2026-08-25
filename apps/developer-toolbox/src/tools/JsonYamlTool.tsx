import { useEffect, useMemo, useState } from "react";
import { downloadTextResult, ToolOutput, ToolTextArea } from "./common";
import {
  convertJsonYaml,
  type JsonYamlDirection,
} from "./jsonYaml";

const DIRECTION_LABELS: Readonly<Record<JsonYamlDirection, string>> = {
  "json-to-yaml": "JSON → YAML",
  "yaml-to-json": "YAML → JSON",
};

export function JsonYamlTool() {
  const [direction, setDirection] = useState<JsonYamlDirection>("json-to-yaml");
  const [input, setInput] = useState("");
  const [actionError, setActionError] = useState<string | null>(null);
  const result = useMemo(() => convertJsonYaml(input, direction), [direction, input]);
  const outputName = direction === "json-to-yaml" ? "converted.yaml" : "converted.json";

  useEffect(() => setActionError(null), [direction, result.output]);

  const copy = () => {
    if (!result.output) return;
    void navigator.clipboard.writeText(result.output)
      .then(() => setActionError(null))
      .catch(() => setActionError("변환 결과를 clipboard에 복사하지 못했습니다."));
  };

  const save = () => {
    if (!result.output) return;
    try {
      downloadTextResult(result.output, outputName);
      setActionError(null);
    } catch {
      setActionError("변환 결과 파일을 저장하지 못했습니다.");
    }
  };

  const useAsOppositeInput = () => {
    if (!result.output) return;
    setInput(result.output);
    setDirection((current) => current === "json-to-yaml" ? "yaml-to-json" : "json-to-yaml");
  };

  return (
    <div className="tool json-yaml-tool">
      <div className="conversion-toolbar" aria-label="변환 방향">
        {(Object.keys(DIRECTION_LABELS) as JsonYamlDirection[]).map((value) => (
          <button
            key={value}
            type="button"
            className={`btn ${direction === value ? "active" : ""}`}
            aria-pressed={direction === value}
            onClick={() => setDirection(value)}
          >
            {DIRECTION_LABELS[value]}
          </button>
        ))}
        <button
          type="button"
          className="btn"
          disabled={!result.output}
          onClick={useAsOppositeInput}
        >
          결과를 반대 방향 입력으로 사용
        </button>
      </div>

      {direction === "yaml-to-json" ? (
        <div className="conversion-notice" role="note">
          <strong>손실 안내</strong>
          JSON은 YAML 주석과 anchor/alias를 표현할 수 없습니다. 주석은 제거되고 alias는 값으로
          확장되며 anchor 이름과 공유 관계는 보존되지 않습니다.
        </div>
      ) : null}

      <div className="io-grid">
        <div className="io-col">
          <div className="io-label">입력 · {direction === "json-to-yaml" ? "JSON" : "YAML 1.2"}</div>
          <ToolTextArea
            aria-label={`${DIRECTION_LABELS[direction]} 입력`}
            className="io-input json-yaml-input"
            placeholder={direction === "json-to-yaml" ? "JSON을 붙여넣으세요..." : "YAML을 붙여넣으세요..."}
            rows={18}
            value={input}
            onValueChange={setInput}
            spellCheck={false}
          />
        </div>
        <div className="io-col">
          <div className="io-label conversion-output-label">
            출력 · {direction === "json-to-yaml" ? "YAML" : "JSON"}
            <span className="conversion-actions">
              <button type="button" className="copy-btn" disabled={!result.output} onClick={copy}>복사</button>
              <button type="button" className="copy-btn" disabled={!result.output} onClick={save}>저장</button>
            </span>
          </div>
          {result.error ? (
            <div className="conversion-error" role="alert">
              <strong>{result.error.message}</strong>
              <span>
                {result.error.line !== null && result.error.column !== null
                  ? `${result.error.line}행 ${result.error.column}열 · `
                  : ""}
                {result.error.code}
              </span>
            </div>
          ) : null}
          <ToolOutput
            ariaLabel={`${DIRECTION_LABELS[direction]} 출력`}
            className="io-output json-yaml-output"
            value={result.output}
            downloadName={outputName}
          />
          {actionError ? <div className="context-action-error" role="alert">{actionError}</div> : null}
        </div>
      </div>
    </div>
  );
}
