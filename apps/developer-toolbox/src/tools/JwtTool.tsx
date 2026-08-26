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
  JWT_LIMITS,
  parseJwt,
  type JwtKeyEncoding,
  type JwtVerificationStatus,
  validateJwtTimes,
} from "./jwt";

const KEY_ENCODINGS: ReadonlyArray<{ value: JwtKeyEncoding; label: string }> = [
  { value: "utf8", label: "UTF-8 text" },
  { value: "hex", label: "Hex" },
  { value: "base64", label: "Base64 (padded)" },
  { value: "base64url", label: "Base64URL (unpadded)" },
];

const STATUS_LABELS: Readonly<Record<JwtVerificationStatus, string>> = {
  unverified: "Unverified — signature has not been checked",
  verified: "Verified — signature and time claims passed",
  invalid_signature: "Invalid — signature did not match",
  invalid_claims: "Invalid — time claims are outside the allowed clock skew",
  error: "Unable to verify",
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
    if (value.length > JWT_LIMITS.maxTokenBytes) return;
    invalidateResult();
    setToken(value);
  };

  const changeKey = (value: string) => {
    if (value.length > JWT_LIMITS.maxKeyTextBytes) return;
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
        Decode displays the header and payload as <strong>unverified</strong>. Verify is explicit and
        supports only HS256, HS384, and HS512 with a raw UTF-8, hex, padded Base64, or unpadded
        Base64URL key. PEM/JWK, RSA, EC, alg=none, token storage, and automatic clipboard actions
        are not supported. Time claims use the current UTC clock with a fixed ±{JWT_LIMITS.clockSkewSeconds}s skew.
      </p>

      <div className="jwt-toolbar">
        <button type="button" className="btn" onClick={decode} disabled={running || !token}>
          Decode
        </button>
        <button type="button" className="btn" onClick={() => void verify()} disabled={running || !token || !key}>
          {running ? "Verifying..." : "Verify signature"}
        </button>
      </div>

      <div className="jwt-key-row">
        <label className="jwt-field">
          Verification key
          <ToolTextField
            aria-label="JWT verification key"
            aria-describedby="jwt-help"
            autoComplete="off"
            inputType="password"
            value={key}
            onValueChange={changeKey}
            disabled={running}
            maxLength={JWT_LIMITS.maxKeyTextBytes}
            spellCheck={false}
          />
        </label>
        <label className="jwt-field">
          Key encoding
          <select
            aria-label="JWT key encoding"
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
        {running ? "Verifying JWT..." : STATUS_LABELS[status]}
      </div>
      {error ? <div className="jwt-error" role="alert">{error}</div> : null}

      <div className="io-grid jwt-grid">
        <div className="io-col">
          <div className="io-label">JWT compact token</div>
          <ToolTextArea
            aria-label="JWT compact token"
            aria-describedby="jwt-help"
            aria-busy={running}
            className="io-input jwt-input"
            placeholder="Paste header.payload.signature..."
            rows={7}
            value={token}
            onValueChange={changeToken}
            disabled={running}
            maxLength={JWT_LIMITS.maxTokenBytes}
            spellCheck={false}
          />
        </div>
        <div className="io-col">
          <div className="io-label">Decoded claims and verification</div>
          <ToolOutput
            ariaLabel="JWT decoded output"
            className={`io-output jwt-output ${error ? "io-error" : ""}`}
            value={output}
            downloadName="dev-toolbox-jwt-result.json"
          />
        </div>
      </div>
    </div>
  );
}
