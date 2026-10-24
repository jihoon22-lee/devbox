import { ToolOutput, ToolTextArea, ToolTextField } from "../frame";
import { MAX_PAYLOAD_BYTES, MAX_WIFI_PASSWORD_BYTES, MAX_WIFI_SSID_BYTES, type WifiRequest } from "../lib/qr";
import { QrOptions } from "./QrOptions";
import { FIXED_ACTION_ERROR, PRESETS } from "./qrConfiguration";
import { useQrState } from "./useQrState";
export function QrTool() {
  const state = useQrState();
  const {
    preset,
    setPreset,
    text,
    setText,
    url,
    setUrl,
    wifi,
    setWifiField,
    result,
    error,
    actionError,
    running,
    composing,
    invalidate,
    copyPng,
    savePng,
    copySvg,
    saveSvg,
  } = state;
  return (
    <div className="tool qr-tool" aria-busy={running}>
      <div className="qr-preset-toolbar" role="group" aria-label="QR 입력 유형">
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

      <QrOptions state={state} />

      <div className="qr-help" role="note">
        모든 생성은 오프라인에서 처리하며 입력·결과를 자동 저장하거나 전송하지 않습니다. 크기는 모듈 단위로 맞춰져
        요청한 최대 크기보다 작아질 수 있습니다. 오류 보정이 높을수록 같은 버전에서 담을 수 있는 데이터가 줄어듭니다.
      </div>

      {preset === "text" ? (
        <div className="qr-input-panel">
          <label htmlFor="qr-text-input">텍스트 페이로드</label>
          <ToolTextArea
            id="qr-text-input"
            aria-label="텍스트 페이로드"
            className="io-input qr-payload-input"
            placeholder="QR로 만들 텍스트를 입력하세요..."
            rows={8}
            value={text}
            onValueChange={(value) => {
              if (withinUtf8Limit(value, MAX_PAYLOAD_BYTES)) invalidate(() => setText(value));
            }}
            fixedActionError={FIXED_ACTION_ERROR}
            disabled={running}
            maxLength={MAX_PAYLOAD_BYTES}
            spellCheck={false}
            onCompositionStart={() => {
              composing.current = true;
            }}
            onCompositionEnd={() => {
              composing.current = false;
            }}
          />
        </div>
      ) : null}

      {preset === "url" ? (
        <div className="qr-input-panel">
          <label htmlFor="qr-url-input">HTTP(S) URL 페이로드</label>
          <ToolTextField
            id="qr-url-input"
            aria-label="HTTP(S) URL 페이로드"
            className="qr-single-input"
            placeholder="https://example.com/..."
            value={url}
            onValueChange={(value) => {
              if (withinUtf8Limit(value, MAX_PAYLOAD_BYTES)) invalidate(() => setUrl(value));
            }}
            fixedActionError={FIXED_ACTION_ERROR}
            disabled={running}
            maxLength={MAX_PAYLOAD_BYTES}
            onCompositionStart={() => {
              composing.current = true;
            }}
            onCompositionEnd={() => {
              composing.current = false;
            }}
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
              onValueChange={(value) => {
                if (withinUtf8Limit(value, MAX_WIFI_SSID_BYTES)) setWifiField("ssid", value);
              }}
              fixedActionError={FIXED_ACTION_ERROR}
              disabled={running}
              maxLength={MAX_WIFI_SSID_BYTES}
              onCompositionStart={() => {
                composing.current = true;
              }}
              onCompositionEnd={() => {
                composing.current = false;
              }}
            />
          </label>
          <label>
            보안
            <select
              aria-label="Wi-Fi 보안"
              value={wifi.security}
              disabled={running}
              onChange={(event) => setWifiField("security", event.currentTarget.value as WifiRequest["security"])}
            >
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
              onValueChange={(value) => {
                if (withinUtf8Limit(value, MAX_WIFI_PASSWORD_BYTES)) setWifiField("password", value);
              }}
              fixedActionError={FIXED_ACTION_ERROR}
              disabled={running}
              maxLength={MAX_WIFI_PASSWORD_BYTES}
              onCompositionStart={() => {
                composing.current = true;
              }}
              onCompositionEnd={() => {
                composing.current = false;
              }}
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
          <div className="qr-field-help">
            SSID는 UTF-8 32바이트, 비밀번호는 63바이트까지입니다. 예약 문자는 QR 형식에 맞게 이스케이프 처리됩니다.
          </div>
        </div>
      ) : null}

      {error ? (
        <div className="qr-error" role="alert">
          {error}
        </div>
      ) : null}
      <div className="qr-status" role="status" aria-live="polite" aria-atomic="true">
        {running
          ? "QR을 생성하는 중입니다."
          : result
            ? `${result.width}px · 버전 ${result.version} · ${result.payloadBytes}바이트`
            : ""}
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
              <button type="button" className="copy-btn" onClick={copyPng}>
                PNG 복사
              </button>
              <button type="button" className="copy-btn" onClick={savePng}>
                PNG 저장
              </button>
            </div>
          </div>
        ) : null}
        <div className="qr-svg-result">
          <div className="io-label">
            SVG 결과
            <span className="conversion-actions">
              <button type="button" className="copy-btn" aria-label="SVG 복사" disabled={!result} onClick={copySvg}>
                복사
              </button>
              <button type="button" className="copy-btn" aria-label="SVG 저장" disabled={!result} onClick={saveSvg}>
                저장
              </button>
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
      {actionError ? (
        <div className="context-action-error" role="alert">
          {actionError}
        </div>
      ) : null}
    </div>
  );
}
function withinUtf8Limit(value: string, maximumBytes: number): boolean {
  return new TextEncoder().encode(value).byteLength <= maximumBytes;
}
