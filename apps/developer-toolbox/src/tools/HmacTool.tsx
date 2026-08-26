import { useEffect, useRef, useState } from "react";
import { hmacGenerate, hmacVerify } from "../api";
import { CopyBtn, ToolOutput, ToolTextArea, ToolTextField } from "./common";
import {
  HMAC_ERROR,
  MAX_HMAC_OUTPUT_CHARS,
  MAX_HMAC_TEXT_BYTES,
  validateHmacRequest,
  type HmacAlgorithm,
  type HmacInputEncoding,
  type HmacOutputEncoding,
} from "./hmac";

type HmacMode = "generate" | "verify";

const ALGORITHMS: ReadonlyArray<{ value: HmacAlgorithm; label: string }> = [
  { value: "sha256", label: "HMAC-SHA-256" },
  { value: "sha384", label: "HMAC-SHA-384" },
  { value: "sha512", label: "HMAC-SHA-512" },
];

const INPUT_ENCODINGS: ReadonlyArray<{ value: HmacInputEncoding; label: string }> = [
  { value: "utf8", label: "UTF-8 text" },
  { value: "hex", label: "Hex" },
  { value: "base64", label: "Base64" },
  { value: "base64url", label: "Base64URL (unpadded)" },
];

const OUTPUT_ENCODINGS: ReadonlyArray<{ value: HmacOutputEncoding; label: string }> = [
  { value: "hex", label: "Hex (lowercase)" },
  { value: "base64", label: "Base64 (padded)" },
  { value: "base64url", label: "Base64URL (unpadded)" },
];

/** HMAC generation and constant-time verification with an in-memory-only UI. */
export function HmacTool() {
  const [mode, setMode] = useState<HmacMode>("generate");
  const [algorithm, setAlgorithm] = useState<HmacAlgorithm>("sha256");
  const [key, setKey] = useState("");
  const [keyEncoding, setKeyEncoding] = useState<HmacInputEncoding>("utf8");
  const [message, setMessage] = useState("");
  const [messageEncoding, setMessageEncoding] = useState<HmacInputEncoding>("utf8");
  const [outputEncoding, setOutputEncoding] = useState<HmacOutputEncoding>("hex");
  const [expectedTag, setExpectedTag] = useState("");
  const [output, setOutput] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const requestId = useRef(0);
  const runningRef = useRef(false);
  const composing = useRef(0);

  useEffect(() => {
    return () => {
      requestId.current += 1;
    };
  }, []);

  const invalidatePending = () => {
    requestId.current += 1;
    runningRef.current = false;
    setRunning(false);
    setOutput("");
    setError(null);
  };

  const onCompositionStart = () => {
    composing.current += 1;
  };

  const onCompositionEnd = () => {
    composing.current = Math.max(0, composing.current - 1);
  };

  const run = async () => {
    if (composing.current > 0 || runningRef.current) return;

    const request = {
      algorithm,
      key,
      keyEncoding,
      message,
      messageEncoding,
      outputEncoding,
    } as const;
    try {
      validateHmacRequest(request);
      if (mode === "verify" && expectedTag.length === 0) throw new Error(HMAC_ERROR);
    } catch {
      setOutput("");
      setError(HMAC_ERROR);
      return;
    }

    const currentRequest = ++requestId.current;
    runningRef.current = true;
    setRunning(true);
    setOutput("");
    setError(null);
    try {
      if (mode === "generate") {
        const generated = await hmacGenerate(request);
        if (requestId.current !== currentRequest) return;
        setOutput(generated);
      } else {
        const valid = await hmacVerify({ ...request, expectedTag });
        if (requestId.current !== currentRequest) return;
        setOutput(valid ? "서명이 일치합니다." : "서명이 일치하지 않습니다.");
      }
    } catch {
      if (requestId.current !== currentRequest) return;
      setError(HMAC_ERROR);
      setOutput("");
    } finally {
      if (requestId.current === currentRequest) {
        runningRef.current = false;
        setRunning(false);
      }
    }
  };

  const selectDisabled = running;
  const helpId = "hmac-help";
  const resultStatus =
    mode === "verify"
      ? output
      : output
        ? "HMAC을 생성했습니다."
        : "";

  return (
    <div className="tool hmac-tool" aria-busy={running}>
      <div className="hmac-toolbar">
        <label className="hmac-field">
          Operation
          <select
            aria-label="HMAC operation"
            aria-describedby={helpId}
            value={mode}
            disabled={selectDisabled}
            onChange={(event) => {
              invalidatePending();
              setMode(event.currentTarget.value as HmacMode);
            }}
          >
            <option value="generate">Generate</option>
            <option value="verify">Verify</option>
          </select>
        </label>
        <label className="hmac-field">
          Algorithm
          <select
            aria-label="HMAC algorithm"
            aria-describedby={helpId}
            value={algorithm}
            disabled={selectDisabled}
            onChange={(event) => {
              invalidatePending();
              setAlgorithm(event.currentTarget.value as HmacAlgorithm);
            }}
          >
            {ALGORITHMS.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </label>
        <label className="hmac-field">
          Output encoding
          <select
            aria-label="HMAC output encoding"
            aria-describedby={helpId}
            value={outputEncoding}
            disabled={selectDisabled}
            onChange={(event) => {
              invalidatePending();
              setOutputEncoding(event.currentTarget.value as HmacOutputEncoding);
            }}
          >
            {OUTPUT_ENCODINGS.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </label>
        <button
          type="button"
          className="btn"
          onClick={() => void run()}
          disabled={running}
        >
          {running ? (mode === "generate" ? "Generating..." : "Verifying...") : mode === "generate" ? "Generate HMAC" : "Verify HMAC"}
        </button>
      </div>

      <div className="hmac-input-grid">
        <label className="hmac-input-field">
          Key
          <ToolTextField
            aria-label="HMAC key"
            aria-describedby={helpId}
            value={key}
            onValueChange={(value) => {
              invalidatePending();
              setKey(value);
            }}
            onCompositionStart={onCompositionStart}
            onCompositionEnd={onCompositionEnd}
            maxLength={MAX_HMAC_TEXT_BYTES}
            disabled={running}
            spellCheck={false}
          />
        </label>
        <label className="hmac-input-field">
          Key encoding
          <select
            aria-label="HMAC key encoding"
            aria-describedby={helpId}
            value={keyEncoding}
            disabled={selectDisabled}
            onChange={(event) => {
              invalidatePending();
              setKeyEncoding(event.currentTarget.value as HmacInputEncoding);
            }}
          >
            {INPUT_ENCODINGS.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </label>
        <label className="hmac-input-field hmac-message-field">
          Message
          <ToolTextArea
            aria-label="HMAC message"
            aria-describedby={helpId}
            className="io-input"
            rows={5}
            value={message}
            onValueChange={(value) => {
              invalidatePending();
              setMessage(value);
            }}
            onCompositionStart={onCompositionStart}
            onCompositionEnd={onCompositionEnd}
            maxLength={MAX_HMAC_TEXT_BYTES}
            disabled={running}
            spellCheck={false}
          />
        </label>
        <label className="hmac-input-field">
          Message encoding
          <select
            aria-label="HMAC message encoding"
            aria-describedby={helpId}
            value={messageEncoding}
            disabled={selectDisabled}
            onChange={(event) => {
              invalidatePending();
              setMessageEncoding(event.currentTarget.value as HmacInputEncoding);
            }}
          >
            {INPUT_ENCODINGS.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </label>
        {mode === "verify" ? (
          <label className="hmac-input-field hmac-expected-field">
            Expected tag ({outputEncoding})
            <ToolTextField
              aria-label="Expected HMAC tag"
              aria-describedby={helpId}
              value={expectedTag}
              onValueChange={(value) => {
                invalidatePending();
                setExpectedTag(value);
              }}
              onCompositionStart={onCompositionStart}
              onCompositionEnd={onCompositionEnd}
              maxLength={MAX_HMAC_OUTPUT_CHARS}
              disabled={running}
              spellCheck={false}
            />
          </label>
        ) : null}
      </div>

      <div id={helpId} className="hmac-help" role="note">
        키와 message는 UTF-8 text, hex, 표준 padded Base64 또는 unpadded Base64URL로 해석합니다.
        결과 encoding도 명시적으로 선택하며 지원 알고리즘은 SHA-256·SHA-384·SHA-512입니다.
        한 필드의 encoded text는 {MAX_HMAC_TEXT_BYTES.toLocaleString()}바이트, decoded key/message는
        1,000,000바이트까지입니다. 검증은 Web Crypto/RustCrypto의 constant-time primitive를
        사용합니다. 키·입력·결과는 현재 화면과 한 번의 작업 메모리에만 존재하며 자동 저장·로그·전송하지 않습니다.
      </div>
      {error ? <div className="hmac-error" role="alert">{error}</div> : null}
      <div className="hmac-status" role="status" aria-live="polite" aria-atomic="true">
        {running ? (mode === "generate" ? "HMAC을 생성하는 중입니다." : "HMAC을 검증하는 중입니다.") : resultStatus}
      </div>

      <div className="io-col hmac-output-col">
        <div className="io-label">
          Result {mode === "generate" && output ? <CopyBtn value={output} /> : null}
        </div>
        <ToolOutput
          className={`io-output ${error ? "io-error" : ""}`}
          value={error || output}
          ariaLabel="HMAC output"
          downloadName="dev-toolbox-hmac-result.txt"
        />
      </div>
    </div>
  );
}
