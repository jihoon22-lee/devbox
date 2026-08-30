import { useEffect, useRef, useState } from "react";
import { verifyJwt } from "../api";
import { ToolOutput, ToolTextArea, ToolTextField } from "./common";
import {
  decodeJwtKey,
  formatJwtDisplay,
  JwtError,
  jwtErrorCode,
  jwtErrorMessage,
  jwtMinimumKeyBytes,
  jwtUtf8ByteLength,
  JWT_LIMITS,
  parseJwt,
  type JwtKeyEncoding,
  type JwtVerificationStatus,
  validateJwtTimes,
} from "./jwt";

const FIXED_CLIPBOARD_ERROR = "JWT 입력을 클립보드에서 읽지 못했습니다.";
const FIXED_OUTPUT_ERROR = "JWT 결과 작업을 완료하지 못했습니다.";

const KEY_ENCODINGS: ReadonlyArray<{ value: JwtKeyEncoding; label: string }> = [
  { value: "utf8", label: "UTF-8 텍스트" },
  { value: "hex", label: "Hex" },
  { value: "base64", label: "Base64 (패딩 포함)" },
  { value: "base64url", label: "Base64URL (패딩 없음)" },
];

const STATUS_LABELS: Readonly<Record<JwtVerificationStatus, string>> = {
  unverified: "검증되지 않음 — 서명을 확인하지 않았습니다",
  verified: "검증됨 — 서명과 시간 클레임을 통과했습니다",
  invalid_signature: "유효하지 않음 — 서명이 일치하지 않습니다",
  invalid_claims: "유효하지 않음 — 시간 클레임이 허용된 시계 오차 범위를 벗어났습니다",
  error: "검증할 수 없음",
};

/**
 * JWT compact decoder and explicit verifier.  Decode is intentionally a
 * separate action from verify: showing claims never implies that a signature
 * was checked, and the key field is never persisted or copied automatically.
 */
export function JwtDecoder() {
  const [token, setToken] = useState("");
  const [key, setKey] = useState("");
  const [keyEncoding, setKeyEncoding] = useState<JwtKeyEncoding>("utf8");
  const [output, setOutput] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<JwtVerificationStatus>("unverified");
  const [running, setRunning] = useState(false);
  const sequence = useRef(0);
  const runningRef = useRef(false);

  useEffect(() => () => {
    sequence.current += 1;
    runningRef.current = false;
  }, []);

  const invalidateResult = () => {
    sequence.current += 1;
    runningRef.current = false;
    setRunning(false);
    setOutput("");
    setError(null);
    setStatus("unverified");
  };

  const changeToken = (value: string) => {
    // The native maxLength attribute also protects normal typing. This guard
    // covers the app-owned context-menu paste path, which updates controlled
    // state programmatically and can otherwise bypass that browser limit.
    if (jwtUtf8ByteLength(value) > JWT_LIMITS.maxTokenBytes) return;
    invalidateResult();
    setToken(value);
  };

  const changeKey = (value: string) => {
    if (jwtUtf8ByteLength(value) > JWT_LIMITS.maxKeyTextBytes) return;
    invalidateResult();
    setKey(value);
  };

  const changeKeyEncoding = (value: JwtKeyEncoding) => {
    invalidateResult();
    setKeyEncoding(value);
  };

  const decode = () => {
    if (runningRef.current) return;
    const current = ++sequence.current;
    runningRef.current = true;
    setRunning(true);
    setError(null);
    setOutput("");
    try {
      const parsed = parseJwt(token);
      if (sequence.current !== current) return;
      setOutput(formatJwtDisplay(parsed));
      setStatus("unverified");
    } catch (caught) {
      if (sequence.current !== current) return;
      setError(jwtErrorMessage(caught, "invalid_input"));
      setStatus("error");
    } finally {
      if (sequence.current === current) {
        runningRef.current = false;
        setRunning(false);
      }
    }
  };

  const verify = async () => {
    if (runningRef.current) return;
    const current = ++sequence.current;
    runningRef.current = true;
    setRunning(true);
    setError(null);
    setOutput("");
    try {
      const parsed = parseJwt(token);
      const nowSeconds = Math.floor(Date.now() / 1000);
      const temporal = validateJwtTimes(parsed.payload, nowSeconds);
      if (!temporal.valid) {
        if (sequence.current !== current) return;
        setOutput(formatJwtDisplay(parsed, {
          status: "invalid_claims",
          verifiedAtSeconds: nowSeconds,
        }));
        setStatus("invalid_claims");
        return;
      }

      const keyBytes = decodeJwtKey(key, keyEncoding);
      if (keyBytes.length < jwtMinimumKeyBytes(parsed.algorithm)) {
        throw new JwtError("key_too_short");
      }
      const valid = await verifyJwt({
        algorithm: parsed.algorithm,
        signingInput: parsed.signingInput,
        signature: parsed.signature,
        key,
        keyEncoding,
      });
      if (sequence.current !== current) return;
      const nextStatus: JwtVerificationStatus = valid ? "verified" : "invalid_signature";
      setOutput(formatJwtDisplay(parsed, {
        status: nextStatus,
        verifiedAtSeconds: nowSeconds,
      }));
      setStatus(nextStatus);
    } catch (caught) {
      if (sequence.current !== current) return;
      // Preserve the fixed key error contract even though the native and
      // browser paths use different exception types internally.
      const code = jwtErrorCode(caught, "verification_failed");
      setError(jwtErrorMessage(caught, code));
      setStatus("error");
    } finally {
      if (sequence.current === current) {
        runningRef.current = false;
        setRunning(false);
      }
    }
  };

  return (
    <div className="tool jwt-tool" aria-busy={running}>
      <p id="jwt-help" className="jwt-help">
        디코드는 헤더와 페이로드를 <strong>검증되지 않은 상태</strong>로 표시합니다. 서명 검증은
        명시적으로 실행하며 raw UTF-8, Hex, 패딩 포함 Base64 또는 패딩 없는 Base64URL 키를
        사용하는 HS256, HS384, HS512만 지원합니다. PEM/JWK, RSA, EC, alg=none, 토큰 저장 및
        클립보드 자동 작업은 지원하지 않습니다. 시간 클레임은 현재 UTC 시각과 고정된 ±{JWT_LIMITS.clockSkewSeconds}초 시계 오차를 사용합니다.
      </p>

      <div className="jwt-toolbar">
        <button type="button" className="btn" onClick={decode} disabled={running || !token}>
          디코드
        </button>
        <button type="button" className="btn" onClick={() => void verify()} disabled={running || !token || !key}>
          {running ? "검증 중..." : "서명 검증"}
        </button>
      </div>

      <div className="jwt-key-row">
        <label className="jwt-field">
          검증 키
          <ToolTextField
            aria-label="JWT 검증 키"
            aria-describedby="jwt-help"
            autoComplete="off"
            inputType="password"
            value={key}
            onValueChange={changeKey}
            disabled={running}
            maxLength={JWT_LIMITS.maxKeyTextBytes}
            maxPasteBytes={JWT_LIMITS.maxKeyTextBytes}
            clipboardErrorMessage={FIXED_CLIPBOARD_ERROR}
            spellCheck={false}
          />
        </label>
        <label className="jwt-field">
          키 인코딩
          <select
            aria-label="JWT 키 인코딩"
            aria-describedby="jwt-help"
            value={keyEncoding}
            onChange={(event) => changeKeyEncoding(event.currentTarget.value as JwtKeyEncoding)}
            disabled={running}
          >
            {KEY_ENCODINGS.map((encoding) => (
              <option key={encoding.value} value={encoding.value}>
                {encoding.label}
              </option>
            ))}
          </select>
        </label>
      </div>

      <div className="jwt-status" role="status" aria-live="polite">
        {running ? "JWT를 검증하는 중..." : STATUS_LABELS[status]}
      </div>
      {error ? <div className="jwt-error" role="alert">{error}</div> : null}

      <div className="io-grid jwt-grid">
        <div className="io-col">
          <div className="io-label">JWT 컴팩트 토큰</div>
          <ToolTextArea
            aria-label="JWT 컴팩트 토큰"
            aria-describedby="jwt-help"
            aria-busy={running}
            className="io-input jwt-input"
            placeholder="header.payload.signature 형식의 토큰을 붙여넣으세요..."
            rows={7}
            value={token}
            onValueChange={changeToken}
            disabled={running}
            maxLength={JWT_LIMITS.maxTokenBytes}
            maxPasteBytes={JWT_LIMITS.maxTokenBytes}
            clipboardErrorMessage={FIXED_CLIPBOARD_ERROR}
            spellCheck={false}
          />
        </div>
        <div className="io-col">
          <div className="io-label">디코드된 클레임 및 검증 결과</div>
          <ToolOutput
            ariaLabel="JWT 디코드 결과"
            className={`io-output jwt-output ${error ? "io-error" : ""}`}
            value={output}
            downloadName="dev-toolbox-jwt-result.json"
            actionErrorMessage={FIXED_OUTPUT_ERROR}
          />
        </div>
      </div>
    </div>
  );
}
