import { MAX_OUTPUT_SIZE, MAX_QUIET_ZONE, MIN_OUTPUT_SIZE, MIN_QUIET_ZONE, type QrErrorCorrection } from "../lib/qr";
import { ERROR_CORRECTION_LEVELS, VERSION_OPTIONS } from "./qrOptions";
import type { QrState } from "./useQrState";
export function QrOptions({ state }: { state: QrState }) {
  const {
    versionText,
    setVersionText,
    errorCorrection,
    setErrorCorrection,
    sizeText,
    setSizeText,
    quietZoneText,
    setQuietZoneText,
    running,
    composing,
    invalidate,
    hasPayload,
    run,
  } = state;
  return (
    <div className="qr-options" role="group" aria-label="QR 출력 옵션">
      <label>
        버전
        <select
          aria-label="QR 버전"
          value={versionText}
          disabled={running}
          onChange={(event) => invalidate(() => setVersionText(event.currentTarget.value))}
        >
          {VERSION_OPTIONS.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
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
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
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
          aria-invalid={
            sizeText !== "" &&
            (!Number.isInteger(Number(sizeText)) ||
              Number(sizeText) < MIN_OUTPUT_SIZE ||
              Number(sizeText) > MAX_OUTPUT_SIZE)
          }
          onCompositionStart={() => {
            composing.current = true;
          }}
          onCompositionEnd={() => {
            composing.current = false;
          }}
          onChange={(event) => invalidate(() => setSizeText(event.currentTarget.value))}
        />
      </label>
      <label>
        여백 (모듈)
        <input
          aria-label="QR 여백"
          type="text"
          inputMode="numeric"
          value={quietZoneText}
          disabled={running}
          aria-invalid={
            quietZoneText !== "" &&
            (!Number.isInteger(Number(quietZoneText)) ||
              Number(quietZoneText) < MIN_QUIET_ZONE ||
              Number(quietZoneText) > MAX_QUIET_ZONE)
          }
          onCompositionStart={() => {
            composing.current = true;
          }}
          onCompositionEnd={() => {
            composing.current = false;
          }}
          onChange={(event) => invalidate(() => setQuietZoneText(event.currentTarget.value))}
        />
      </label>
      <button
        type="button"
        className="btn qr-generate-button"
        disabled={running || !hasPayload}
        onClick={() => void run()}
      >
        {running ? "생성 중..." : "QR 생성"}
      </button>
    </div>
  );
}
