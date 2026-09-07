import { useEffect, useMemo, useState } from "react";
import { downloadTextResult, ToolOutput, ToolTextArea } from "./common";
import {
  convertByteEncoding,
  type ByteEncoding,
} from "./byteCodec";

const ENCODING_LABELS: Readonly<Record<ByteEncoding, string>> = {
  utf8: "UTF-8 텍스트",
  hex: "Hex 원시 바이트",
  base64: "Base64",
  base64url: "Base64URL",
};

const OUTPUT_NAMES: Readonly<Record<ByteEncoding, string>> = {
  utf8: "converted.txt",
  hex: "converted.hex.txt",
  base64: "converted.base64.txt",
  base64url: "converted.base64url.txt",
};

export function ByteCodecTool() {
  const [source, setSource] = useState<ByteEncoding>("utf8");
  const [target, setTarget] = useState<ByteEncoding>("base64");
  const [input, setInput] = useState("");
  const [actionError, setActionError] = useState<string | null>(null);
  const result = useMemo(() => convertByteEncoding(input, source, target), [input, source, target]);

  useEffect(() => setActionError(null), [result.output, source, target]);

  const copy = () => {
    if (!result.output) return;
    void navigator.clipboard.writeText(result.output)
      .then(() => setActionError(null))
      .catch(() => setActionError("변환 결과를 클립보드에 복사하지 못했습니다."));
  };

  const save = () => {
    if (!result.output) return;
    try {
      downloadTextResult(result.output, OUTPUT_NAMES[target]);
      setActionError(null);
    } catch {
      setActionError("변환 결과 파일을 저장하지 못했습니다.");
    }
  };

  const swap = () => {
    if (!result.output) return;
    setInput(result.output);
    setSource(target);
    setTarget(source);
  };

  return (
    <div className="tool byte-codec-tool">
      <div className="codec-selectors" aria-label="바이트 변환 형식">
        <label>
          입력 형식
          <select
            aria-label="입력 형식"
            value={source}
            onChange={(event) => setSource(event.currentTarget.value as ByteEncoding)}
          >
            {(Object.keys(ENCODING_LABELS) as ByteEncoding[]).map((encoding) => (
              <option key={encoding} value={encoding}>{ENCODING_LABELS[encoding]}</option>
            ))}
          </select>
        </label>
        <span aria-hidden="true">→</span>
        <label>
          출력 형식
          <select
            aria-label="출력 형식"
            value={target}
            onChange={(event) => setTarget(event.currentTarget.value as ByteEncoding)}
          >
            {(Object.keys(ENCODING_LABELS) as ByteEncoding[]).map((encoding) => (
              <option key={encoding} value={encoding}>{ENCODING_LABELS[encoding]}</option>
            ))}
          </select>
        </label>
        <button type="button" className="btn" disabled={!result.output} onClick={swap}>
          결과로 입출력 교환
        </button>
      </div>

      <div className="conversion-notice" role="note">
        <strong>표현 안내</strong>
        UTF-8은 텍스트를 바이트로 인코딩합니다. 임의의 원시 바이트는 Hex/Base64 계열로 보존합니다.
        Hex/Base64 입력의 ASCII 공백은 무시하고 Base64 계열은 올바른 끝 패딩 생략을
        허용합니다. 유효하지 않은 UTF-8은 대체 문자로 바꾸지 않고 바이트 위치를 표시합니다.
        Base64는 암호화가 아니며 입력과 결과를 자동으로 저장하거나 전송하지 않습니다.
      </div>

      <div className="io-grid">
        <div className="io-col">
          <div className="io-label">입력 · {ENCODING_LABELS[source]}</div>
          <ToolTextArea
            aria-label={`${ENCODING_LABELS[source]} 입력`}
            className="io-input byte-codec-input"
            placeholder={`${ENCODING_LABELS[source]} 값을 붙여넣으세요...`}
            rows={18}
            value={input}
            onValueChange={setInput}
            spellCheck={false}
          />
        </div>
        <div className="io-col">
          <div className="io-label conversion-output-label">
            출력 · {ENCODING_LABELS[target]} · {result.byteLength.toLocaleString()}바이트
            <span className="conversion-actions">
              <button type="button" className="copy-btn" disabled={!result.output} onClick={copy}>복사</button>
              <button type="button" className="copy-btn" disabled={!result.output} onClick={save}>저장</button>
            </span>
          </div>
          {result.error ? (
            <div className="conversion-error" role="alert">
              <strong>{result.error.message}</strong>
              <span>
                {result.error.position !== null && result.error.unit !== null
                  ? `${result.error.position}번째 ${result.error.unit === "byte" ? "바이트" : "문자"} · `
                  : ""}
                {result.error.code}
              </span>
            </div>
          ) : null}
          <ToolOutput
            ariaLabel={`${ENCODING_LABELS[target]} 출력`}
            className="io-output byte-codec-output"
            value={result.output}
            downloadName={OUTPUT_NAMES[target]}
          />
          {actionError ? <div className="context-action-error" role="alert">{actionError}</div> : null}
        </div>
      </div>
    </div>
  );
}
