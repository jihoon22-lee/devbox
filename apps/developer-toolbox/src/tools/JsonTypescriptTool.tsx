import { useEffect, useMemo, useState } from "react";
import { downloadTextResult, ToolOutput, ToolTextArea, ToolTextField } from "./common";
import { convertJsonToTypescript } from "./jsonTypescript";

const DEFAULT_ROOT_TYPE_NAME = "RootObject";

export function JsonTypescriptTool() {
  const [rootTypeName, setRootTypeName] = useState(DEFAULT_ROOT_TYPE_NAME);
  const [input, setInput] = useState("");
  const [actionError, setActionError] = useState<string | null>(null);
  const result = useMemo(
    () => convertJsonToTypescript(input, rootTypeName),
    [input, rootTypeName],
  );

  useEffect(() => setActionError(null), [input, rootTypeName]);

  const copy = () => {
    if (!result.output) return;
    void navigator.clipboard.writeText(result.output)
      .then(() => setActionError(null))
      .catch(() => setActionError("TypeScript 결과를 clipboard에 복사하지 못했습니다."));
  };

  const save = () => {
    if (!result.output) return;
    try {
      downloadTextResult(result.output, `${rootTypeName}.ts`);
      setActionError(null);
    } catch {
      setActionError("TypeScript 결과 파일을 저장하지 못했습니다.");
    }
  };

  return (
    <div className="tool json-typescript-tool">
      <div className="json-typescript-toolbar">
        <label>
          Root type 이름
          <ToolTextField
            aria-label="Root type 이름"
            className="json-typescript-name"
            value={rootTypeName}
            onValueChange={setRootTypeName}
            spellCheck={false}
          />
        </label>
        <button type="button" className="btn" disabled={!result.output} onClick={copy}>
          결과 복사
        </button>
        <button type="button" className="btn" disabled={!result.output} onClick={save}>
          .ts 저장
        </button>
      </div>

      <div className="conversion-notice" role="note">
        <strong>추론 안내</strong>
        배열의 object 표본은 하나의 구조로 병합하며 누락된 속성은 optional로, null은 union으로
        보존합니다. 빈 배열의 원소는 unknown으로 생성합니다. 값 자체는 코드에 포함하지 않으며
        입력과 결과를 자동 저장하거나 외부로 전송하지 않습니다.
      </div>

      <div className="io-grid">
        <div className="io-col">
          <div className="io-label">입력 · strict JSON</div>
          <ToolTextArea
            aria-label="JSON → TypeScript 입력"
            className="io-input json-typescript-input"
            placeholder={'예: {"users":[{"id":1,"name":"Ada"},{"id":2}]}'}
            rows={18}
            value={input}
            onValueChange={setInput}
            spellCheck={false}
          />
        </div>
        <div className="io-col">
          <div className="io-label conversion-output-label">출력 · TypeScript</div>
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
            ariaLabel="JSON → TypeScript 출력"
            className="io-output json-typescript-output"
            value={result.output}
            downloadName={`${rootTypeName || DEFAULT_ROOT_TYPE_NAME}.ts`}
          />
          {actionError ? <div className="context-action-error" role="alert">{actionError}</div> : null}
        </div>
      </div>
    </div>
  );
}
