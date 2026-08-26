import { useEffect, useRef, useState } from "react";
import { generateQr } from "../api";
import { downloadBinaryResult, downloadTextResult, ToolOutput, ToolTextArea, ToolTextField } from "./common";
import {
  MAX_OUTPUT_SIZE,
  MAX_PAYLOAD_BYTES,
  MAX_QUIET_ZONE,
  MAX_WIFI_PASSWORD_BYTES,
  MAX_WIFI_SSID_BYTES,
  MIN_OUTPUT_SIZE,
  MIN_QUIET_ZONE,
  QR_ERROR_MESSAGES,
  type GenerateQrRequest,
  type QrErrorCorrection,
  type QrPreset,
  type QrResult,
  type QrVersion,
  type WifiRequest,
  QrGenerationError,
} from "./qr";

const ERROR_CORRECTION_LEVELS: ReadonlyArray<{ value: QrErrorCorrection; label: string }> = [
  { value: "L", label: "L · 낮음" },
  { value: "M", label: "M · 중간" },
  { value: "Q", label: "Q · 높음" },
  { value: "H", label: "H · 최고" },
];

const PRESETS: ReadonlyArray<{ value: QrPreset; label: string }> = [
  { value: "text", label: "텍스트" },
  { value: "url", label: "URL" },
  { value: "wifi", label: "Wi-Fi" },
];

const VERSION_OPTIONS: ReadonlyArray<{ value: string; label: string }> = [
  { value: "auto", label: "자동 (최소 버전)" },
  ...Array.from({ length: 40 }, (_, index) => {
    const version = index + 1;
    return { value: String(version), label: `버전 ${version}` };
  }),
];

const FIXED_ACTION_ERROR = "QR 작업을 완료하지 못했습니다.";

const EMPTY_WIFI: WifiRequest = {
  ssid: "",
  password: "",
  security: "WPA",
  hidden: false,
};

export function QrTool() {
  const [preset, setPreset] = useState<QrPreset>("text");
  const [text, setText] = useState("");
  const [url, setUrl] = useState("");
  const [wifi, setWifi] = useState<WifiRequest>(EMPTY_WIFI);
  const [versionText, setVersionText] = useState("auto");
  const [errorCorrection, setErrorCorrection] = useState<QrErrorCorrection>("M");
  const [sizeText, setSizeText] = useState("512");
  const [quietZoneText, setQuietZoneText] = useState("4");
  const [result, setResult] = useState<QrResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const requestId = useRef(0);
  const runningRef = useRef(false);
  const composing = useRef(false);

  useEffect(() => () => {
    requestId.current += 1;
  }, []);

  const invalidate = (change: () => void) => {
    requestId.current += 1;
    runningRef.current = false;
    setRunning(false);
    setResult(null);
    setError(null);
    setActionError(null);
    change();
  };

  const hasPayload = preset === "text"
    ? text.length > 0
    : preset === "url"
      ? url.length > 0
      : wifi.ssid.length > 0;

  const run = async () => {
    if (composing.current || runningRef.current || !hasPayload) return;
    const size = Number(sizeText);
    const quietZone = Number(quietZoneText);
    const versionValue = versionText === "auto" ? null : Number(versionText);
    const request: GenerateQrRequest = {
      preset,
      text: preset === "text" ? text : undefined,
      url: preset === "url" ? url : undefined,
      wifi: preset === "wifi" ? wifi : undefined,
      version: versionValue === null ? null : versionValue as QrVersion,
      errorCorrection,
      size,
      quietZone,
    };
    const currentRequest = ++requestId.current;
    runningRef.current = true;
    setRunning(true);
    setError(null);
    setActionError(null);
    try {
      const generated = await generateQr(request);
      if (requestId.current !== currentRequest) return;
      setResult(generated);
    } catch (cause) {
      if (requestId.current !== currentRequest) return;
      setResult(null);
      setError(cause instanceof QrGenerationError ? cause.message : QR_ERROR_MESSAGES.render);
    } finally {
      if (requestId.current === currentRequest) {
        runningRef.current = false;
        setRunning(false);
      }
    }
  };

  const setWifiField = <K extends keyof WifiRequest>(field: K, value: WifiRequest[K]) => {
    invalidate(() => setWifi((current) => ({ ...current, [field]: value })));
  };

  const copySvg = () => {
    if (!result || !navigator.clipboard?.writeText) {
      setActionError(FIXED_ACTION_ERROR);
      return;
    }
    try {
      void navigator.clipboard.writeText(result.svg)
        .then(() => setActionError(null))
        .catch(() => setActionError(FIXED_ACTION_ERROR));
    } catch {
      setActionError(FIXED_ACTION_ERROR);
    }
  };

  const copyPng = () => {
    if (!result || typeof ClipboardItem === "undefined" || !navigator.clipboard?.write) {
      setActionError("이 환경에서는 PNG clipboard를 사용할 수 없습니다. PNG 저장을 사용하세요.");
      return;
    }
    try {
      const bytes = decodeBase64(result.pngBase64);
      const item = new ClipboardItem({ "image/png": new Blob([bytes], { type: "image/png" }) });
      void navigator.clipboard.write([item])
        .then(() => setActionError(null))
        .catch(() => setActionError("PNG clipboard를 사용할 수 없습니다. PNG 저장을 사용하세요."));
    } catch {
      setActionError(FIXED_ACTION_ERROR);
    }
  };

  const saveSvg = () => {
    if (!result) return;
    try {
      downloadTextResult(result.svg, "devbox-qr.svg");
      setActionError(null);
    } catch {
      setActionError(FIXED_ACTION_ERROR);
    }
  };

  const savePng = () => {
    if (!result) return;
    try {
      downloadBinaryResult(result.pngBase64, "devbox-qr.png", "image/png");
      setActionError(null);
    } catch {
      setActionError(FIXED_ACTION_ERROR);
    }
  };

  return (
    <div className="tool qr-tool" aria-busy={running}>
      <div className="qr-preset-toolbar" aria-label="QR 입력 유형">
        {PRESETS.map((option) => (
          <button
            key={option.value}
            type="button"
            className={`btn ${preset === option.value ? "active" : ""}`}
            aria-pressed={preset === option.value}
            disabled={running}
            onClick={() => invalidate(() => setPreset(option.value))}
          >
            {option.label}
          </button>
        ))}
      </div>

      <div className="qr-options" aria-label="QR 출력 옵션">
        <label>
          버전
          <select
            aria-label="QR 버전"
            value={versionText}
            disabled={running}
            onChange={(event) => invalidate(() => setVersionText(event.currentTarget.value))}
          >
            {VERSION_OPTIONS.map((option) => (
              <option key={option.value} value={option.value}>{option.label}</option>
            ))}
          </select>
        </label>
        <label>
          오류 보정
          <select
            aria-label="오류 보정 수준"
            value={errorCorrection}
            disabled={running}
            onChange={(event) => invalidate(() => setErrorCorrection(event.currentTarget.value as QrErrorCorrection))}
          >
            {ERROR_CORRECTION_LEVELS.map((option) => (
              <option key={option.value} value={option.value}>{option.label}</option>
            ))}
          </select>
        </label>
        <label>
          최대 크기 (px)
          <input
            aria-label="QR 최대 크기"
            type="text"
            inputMode="numeric"
            value={sizeText}
            disabled={running}
            aria-invalid={sizeText !== "" && (!Number.isInteger(Number(sizeText)) || Number(sizeText) < MIN_OUTPUT_SIZE || Number(sizeText) > MAX_OUTPUT_SIZE)}
            onCompositionStart={() => { composing.current = true; }}
            onCompositionEnd={() => { composing.current = false; }}
            onChange={(event) => invalidate(() => setSizeText(event.currentTarget.value))}
          />
        </label>
        <label>
          Quiet zone (modules)
          <input
            aria-label="QR quiet zone"
            type="text"
            inputMode="numeric"
            value={quietZoneText}
            disabled={running}
            aria-invalid={quietZoneText !== "" && (!Number.isInteger(Number(quietZoneText)) || Number(quietZoneText) < MIN_QUIET_ZONE || Number(quietZoneText) > MAX_QUIET_ZONE)}
            onCompositionStart={() => { composing.current = true; }}
            onCompositionEnd={() => { composing.current = false; }}
            onChange={(event) => invalidate(() => setQuietZoneText(event.currentTarget.value))}
          />
        </label>
        <button type="button" className="btn qr-generate-button" disabled={running || !hasPayload} onClick={() => void run()}>
          {running ? "생성 중..." : "QR 생성"}
        </button>
      </div>

      <div className="qr-help" role="note">
        모든 생성은 오프라인에서 처리하며 입력·결과를 자동 저장하거나 전송하지 않습니다. 크기는
        모듈 단위로 맞춰져 요청한 최대 크기보다 작아질 수 있습니다. 오류 보정이 높을수록 같은
        버전에서 담을 수 있는 데이터가 줄어듭니다.
      </div>

      {preset === "text" ? (
        <div className="qr-input-panel">
          <label htmlFor="qr-text-input">텍스트 payload</label>
          <ToolTextArea
            id="qr-text-input"
            aria-label="텍스트 payload"
            className="io-input qr-payload-input"
            placeholder="QR로 만들 텍스트를 입력하세요..."
            rows={8}
            value={text}
            onValueChange={(value) => invalidate(() => setText(value))}
            fixedActionError={FIXED_ACTION_ERROR}
            maxLength={MAX_PAYLOAD_BYTES}
            spellCheck={false}
            onCompositionStart={() => { composing.current = true; }}
            onCompositionEnd={() => { composing.current = false; }}
          />
        </div>
      ) : null}

      {preset === "url" ? (
        <div className="qr-input-panel">
          <label htmlFor="qr-url-input">HTTP(S) URL payload</label>
          <ToolTextField
            id="qr-url-input"
            aria-label="HTTP(S) URL payload"
            className="qr-single-input"
            placeholder="https://example.com/..."
            value={url}
            onValueChange={(value) => invalidate(() => setUrl(value))}
            fixedActionError={FIXED_ACTION_ERROR}
            maxLength={MAX_PAYLOAD_BYTES}
            onCompositionStart={() => { composing.current = true; }}
            onCompositionEnd={() => { composing.current = false; }}
          />
          <div className="qr-field-help">외부 요청 없이 입력한 HTTP 또는 HTTPS 문자열만 QR에 넣습니다.</div>
        </div>
      ) : null}

      {preset === "wifi" ? (
        <div className="qr-wifi-panel">
          <label>
            SSID
            <ToolTextField
              aria-label="Wi-Fi SSID"
              value={wifi.ssid}
              onValueChange={(value) => setWifiField("ssid", value)}
              fixedActionError={FIXED_ACTION_ERROR}
              maxLength={MAX_WIFI_SSID_BYTES}
              onCompositionStart={() => { composing.current = true; }}
              onCompositionEnd={() => { composing.current = false; }}
            />
          </label>
          <label>
            보안
            <select aria-label="Wi-Fi 보안" value={wifi.security} disabled={running} onChange={(event) => setWifiField("security", event.currentTarget.value as WifiRequest["security"])}>
              <option value="WPA">WPA/WPA2</option>
              <option value="WEP">WEP</option>
              <option value="nopass">암호 없음</option>
            </select>
          </label>
          <label>
            비밀번호
            <ToolTextField
              aria-label="Wi-Fi 비밀번호"
              type="password"
              value={wifi.password}
              onValueChange={(value) => setWifiField("password", value)}
              fixedActionError={FIXED_ACTION_ERROR}
              maxLength={MAX_WIFI_PASSWORD_BYTES}
              onCompositionStart={() => { composing.current = true; }}
              onCompositionEnd={() => { composing.current = false; }}
            />
          </label>
          <label className="qr-checkbox">
            <input
              type="checkbox"
              aria-label="숨겨진 Wi-Fi 네트워크"
              checked={wifi.hidden}
              disabled={running}
              onChange={(event) => setWifiField("hidden", event.currentTarget.checked)}
            />
            숨겨진 네트워크
          </label>
          <div className="qr-field-help">SSID는 UTF-8 32바이트, 비밀번호는 63바이트까지입니다. 예약 문자는 QR 형식에 맞게 escape됩니다.</div>
        </div>
      ) : null}

      {error ? <div className="qr-error" role="alert">{error}</div> : null}
      <div className="qr-status" role="status" aria-live="polite" aria-atomic="true">
        {running ? "QR을 생성하는 중입니다." : result ? `${result.width}px · 버전 ${result.version} · ${result.payloadBytes}바이트` : ""}
      </div>

      <section className="qr-result" aria-label="QR 결과">
        {result ? (
          <div className="qr-preview-card">
            <img
              className="qr-preview-image"
              src={`data:image/svg+xml;charset=utf-8,${encodeURIComponent(result.svg)}`}
              alt="생성된 QR 코드 미리보기"
            />
            <div className="qr-preview-actions">
              <button type="button" className="copy-btn" onClick={copyPng}>PNG 복사</button>
              <button type="button" className="copy-btn" onClick={savePng}>PNG 저장</button>
            </div>
          </div>
        ) : null}
        <div className="qr-svg-result">
          <div className="io-label">
            SVG 결과
            <span className="conversion-actions">
              <button type="button" className="copy-btn" disabled={!result} onClick={copySvg}>복사</button>
              <button type="button" className="copy-btn" disabled={!result} onClick={saveSvg}>저장</button>
            </span>
          </div>
          <ToolOutput
            ariaLabel="QR SVG 결과"
            className="io-output qr-svg-output"
            value={result?.svg ?? ""}
            downloadName="devbox-qr.svg"
            fixedActionError={FIXED_ACTION_ERROR}
          />
        </div>
      </section>
      {actionError ? <div className="context-action-error" role="alert">{actionError}</div> : null}
    </div>
  );
}

function decodeBase64(value: string): Uint8Array {
  const binary = atob(value);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
  return bytes;
}
