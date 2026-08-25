import { useEffect, useMemo, useState } from "react";
import { downloadTextResult, ToolOutput, ToolTextField } from "./common";
import {
  convertRadix,
  type RadixInputMode,
  type RadixOutputs,
} from "./radix";

const INPUT_MODES: ReadonlyArray<{ value: RadixInputMode; label: string }> = [
  { value: "auto", label: "자동 · prefix 또는 10진수" },
  { value: "2", label: "2진수" },
  { value: "8", label: "8진수" },
  { value: "10", label: "10진수" },
  { value: "16", label: "16진수" },
];

const OUTPUT_ROWS: ReadonlyArray<{
  key: keyof RadixOutputs;
  label: string;
  filename: string;
}> = [
  { key: "binary", label: "BIN · 2진수", filename: "converted.bin.txt" },
  { key: "octal", label: "OCT · 8진수", filename: "converted.oct.txt" },
  { key: "decimal", label: "DEC · 10진수", filename: "converted.dec.txt" },
  { key: "hexadecimal", label: "HEX · 16진수", filename: "converted.hex.txt" },
];

function combinedOutput(outputs: RadixOutputs): string {
  return [
    `BIN ${outputs.binary}`,
    `OCT ${outputs.octal}`,
    `DEC ${outputs.decimal}`,
    `HEX ${outputs.hexadecimal}`,
  ].join("\n");
}

export function RadixTool() {
  const [mode, setMode] = useState<RadixInputMode>("auto");
  const [input, setInput] = useState("");
  const [actionError, setActionError] = useState<string | null>(null);
  const result = useMemo(() => convertRadix(input, mode), [input, mode]);

  useEffect(() => setActionError(null), [input, mode]);

  const copy = (value: string) => {
    void navigator.clipboard.writeText(value)
      .then(() => setActionError(null))
      .catch(() => setActionError("변환 결과를 clipboard에 복사하지 못했습니다."));
  };

  const saveAll = () => {
    if (!result.outputs) return;
    try {
      downloadTextResult(combinedOutput(result.outputs), "radix-conversion.txt");
      setActionError(null);
    } catch {
      setActionError("진법 변환 결과 파일을 저장하지 못했습니다.");
    }
  };

  return (
    <div className="tool radix-tool">
      <div className="radix-input-row">
        <label>
          입력 진법
          <select
            aria-label="입력 진법"
            value={mode}
            onChange={(event) => setMode(event.currentTarget.value as RadixInputMode)}
          >
            {INPUT_MODES.map((option) => (
              <option key={option.value} value={option.value}>{option.label}</option>
            ))}
          </select>
        </label>
        <ToolTextField
          aria-label="진법 변환 입력"
          className="radix-input"
          placeholder="예: -0x2a"
          value={input}
          onValueChange={setInput}
          spellCheck={false}
        />
        <button type="button" className="btn" disabled={!result.outputs} onClick={saveAll}>
          전체 결과 저장
        </button>
      </div>

      <div className="conversion-notice" role="note">
        <strong>정수 안내</strong>
        부호는 prefix 앞에 쓰고 자동 모드는 0b, 0o, 0x를 감지하며 나머지는 10진수로 읽습니다.
        결과는 signed magnitude이고 two&apos;s complement 해석은 하지 않습니다. 절댓값은 256bit로
        제한하며 내부 공백과 digit separator는 허용하지 않습니다. 입력과 결과는 자동으로
        저장하거나 전송하지 않습니다.
      </div>

      {result.error ? (
        <div className="conversion-error radix-error" role="alert">
          <strong>{result.error.message}</strong>
          <span>
            {result.error.position !== null ? `${result.error.position}번째 문자 · ` : ""}
            {result.error.code}
          </span>
        </div>
      ) : null}

      {result.metadata && result.outputs ? (
        <>
          <div className="radix-meta">
            입력 {result.metadata.inputBase}진수 · {result.metadata.digitCount} digits · {result.metadata.bitLength} bits
            <button type="button" className="copy-btn" onClick={() => copy(combinedOutput(result.outputs!))}>
              전체 복사
            </button>
          </div>
          <div className="radix-results">
            {OUTPUT_ROWS.map((row) => {
              const value = result.outputs![row.key];
              return (
                <div className="radix-result-row" key={row.key}>
                  <span className="radix-result-label">{row.label}</span>
                  <ToolOutput
                    ariaLabel={`${row.label} 출력`}
                    className="io-output radix-output"
                    value={value}
                    downloadName={row.filename}
                  />
                  <button type="button" className="copy-btn" onClick={() => copy(value)}>
                    {row.label} 복사
                  </button>
                </div>
              );
            })}
          </div>
        </>
      ) : null}

      {actionError ? <div className="context-action-error" role="alert">{actionError}</div> : null}
    </div>
  );
}
