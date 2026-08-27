import qrcode from "qrcode-generator";

export const MAX_PAYLOAD_BYTES = 4_096;
export const MAX_WIFI_SSID_BYTES = 32;
export const MAX_WIFI_PASSWORD_BYTES = 63;
export const MIN_OUTPUT_SIZE = 64;
export const MAX_OUTPUT_SIZE = 2_048;
export const MIN_QUIET_ZONE = 4;
export const MAX_QUIET_ZONE = 16;
const MAX_VERSION = 40;
const MAX_MODULE_SCALE = 64;
const MAX_BINARY_OUTPUT_BYTES = 4 * 1024 * 1024;
const MAX_BINARY_OUTPUT_BASE64_LENGTH = MAX_BINARY_OUTPUT_BYTES;
const MAX_SVG_OUTPUT_BYTES = 4 * 1024 * 1024;

export const QR_ERROR_MESSAGES = {
  emptyInput: "QR 입력은 비어 있을 수 없습니다.",
  inputTooLong: "QR 입력이 너무 깁니다.",
  invalidInput: "QR 입력 형식이 올바르지 않습니다.",
  invalidWifi: "Wi-Fi 설정이 올바르지 않습니다.",
  invalidVersion: "QR 버전이 올바르지 않습니다.",
  invalidEc: "QR 오류 보정 수준이 올바르지 않습니다.",
  invalidSize: "QR 크기가 올바르지 않습니다.",
  invalidQuietZone: "QR 여백이 올바르지 않습니다.",
  smallSize: "QR 크기가 버전과 여백에 비해 작습니다.",
  capacity: "QR 용량을 초과했습니다. 버전 또는 오류 보정 수준을 조정하세요.",
  render: "QR 이미지를 생성하지 못했습니다.",
} as const;

export type QrErrorCode = keyof typeof QR_ERROR_MESSAGES;
export type QrPreset = "text" | "url" | "wifi";
export type QrErrorCorrection = "L" | "M" | "Q" | "H";
export type QrVersion = 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14 | 15 | 16 | 17 | 18 | 19 | 20 | 21 | 22 | 23 | 24 | 25 | 26 | 27 | 28 | 29 | 30 | 31 | 32 | 33 | 34 | 35 | 36 | 37 | 38 | 39 | 40;

export interface WifiRequest {
  ssid: string;
  password: string;
  security: "WPA" | "WEP" | "nopass";
  hidden: boolean;
}

export interface GenerateQrRequest {
  preset: QrPreset;
  text?: string;
  url?: string;
  wifi?: WifiRequest;
  version: QrVersion | null;
  errorCorrection: QrErrorCorrection;
  size: number;
  quietZone: number;
}

export interface QrResult {
  svg: string;
  pngBase64: string;
  width: number;
  version: number;
  modules: number;
  quietZone: number;
  payloadBytes: number;
}

export class QrGenerationError extends Error {
  readonly code: QrErrorCode;

  constructor(code: QrErrorCode) {
    super(QR_ERROR_MESSAGES[code]);
    this.name = "QrGenerationError";
    this.code = code;
  }
}

interface PreparedPayload {
  text: string;
  bytes: Uint8Array;
}

/** Browser fallback with the same validated payload boundary as the native command. */
export async function generateQr(request: GenerateQrRequest): Promise<QrResult> {
  if (!request || typeof request !== "object") {
    throw new QrGenerationError("invalidInput");
  }
  if (!(request.preset === "text" || request.preset === "url" || request.preset === "wifi")) {
    throw new QrGenerationError("invalidInput");
  }
  const scaleInfo = validateDimensions(request);
  const prepared = preparePayload(request);
  if (scaleInfo instanceof QrGenerationError) throw scaleInfo;
  if (prepared instanceof QrGenerationError) throw prepared;
  const errorCorrection = validateErrorCorrection(request.errorCorrection);
  if (errorCorrection instanceof QrGenerationError) throw errorCorrection;

  // qrcode-generator's default encoder is configurable; explicitly use the
  // platform UTF-8 encoder so browser and native byte counts agree.
  qrcode.stringToBytes = (value) => Array.from(new TextEncoder().encode(value));
  let code: ReturnType<typeof qrcode>;
  try {
    code = qrcode(request.version ?? 0, errorCorrection);
    code.addData(prepared.text, "Byte");
    code.make();
  } catch {
    throw new QrGenerationError("capacity");
  }

  const modules = code.getModuleCount();
  const totalModules = modules + request.quietZone * 2;
  const scale = Math.min(Math.floor(request.size / totalModules), MAX_MODULE_SCALE);
  if (scale < 1) throw new QrGenerationError("smallSize");
  const width = totalModules * scale;
  const svg = renderSvg(code, scale, request.quietZone, width);
  const pngBase64 = renderPng(code, scale, request.quietZone, width);
  return {
    svg,
    pngBase64,
    width,
    version: Math.floor((modules - 17) / 4),
    modules,
    quietZone: request.quietZone,
    payloadBytes: prepared.bytes.length,
  };
}

function preparePayload(request: GenerateQrRequest): PreparedPayload | QrGenerationError {
  let text: string;
  switch (request.preset) {
    case "text":
      if (request.text === undefined || request.text === null || request.text === "") {
        return new QrGenerationError("emptyInput");
      }
      if (typeof request.text !== "string") return new QrGenerationError("invalidInput");
      text = request.text;
      break;
    case "url":
      if (request.url === undefined || request.url === null || request.url === "") {
        return new QrGenerationError("emptyInput");
      }
      if (typeof request.url !== "string") return new QrGenerationError("invalidInput");
      text = request.url;
      break;
    case "wifi":
      if (!request.wifi) return new QrGenerationError("invalidWifi");
      {
        const wifiPayload = buildWifiPayload(request.wifi);
        if (wifiPayload === null) return new QrGenerationError("invalidWifi");
        text = wifiPayload;
      }
      break;
    default:
      return new QrGenerationError("invalidInput");
  }

  const bytes = encodeUtf8(text);
  if (bytes === null) return new QrGenerationError("invalidInput");
  if (bytes.length > MAX_PAYLOAD_BYTES) return new QrGenerationError("inputTooLong");
  if (request.preset === "url" && !isSafeHttpUrl(text)) return new QrGenerationError("invalidInput");
  return { text, bytes };
}

function validateDimensions(request: GenerateQrRequest): true | QrGenerationError {
  if (request.version !== null && (!Number.isInteger(request.version) || request.version < 1 || request.version > MAX_VERSION)) {
    return new QrGenerationError("invalidVersion");
  }
  if (!Number.isInteger(request.size) || request.size < MIN_OUTPUT_SIZE || request.size > MAX_OUTPUT_SIZE) {
    return new QrGenerationError("invalidSize");
  }
  if (!Number.isInteger(request.quietZone) || request.quietZone < MIN_QUIET_ZONE || request.quietZone > MAX_QUIET_ZONE) {
    return new QrGenerationError("invalidQuietZone");
  }
  return true;
}

function validateErrorCorrection(value: string): QrErrorCorrection | QrGenerationError {
  if (value === "L" || value === "M" || value === "Q" || value === "H") return value;
  return new QrGenerationError("invalidEc");
}

function isSafeHttpUrl(value: string): boolean {
  const scheme = /^https?:\/\//iu.exec(value);
  if (!scheme) return false;
  if ([...value].some((character) => /[\p{White_Space}\p{Cc}]/u.test(character) || character === "\ufeff")) return false;
  const authority = value.slice(scheme[0].length).split(/[/?#]/u, 1)[0] ?? "";
  return authority.length > 0;
}

export function buildWifiPayload(wifi: WifiRequest): string | null {
  if (
    !wifi ||
    typeof wifi !== "object" ||
    typeof wifi.ssid !== "string" ||
    typeof wifi.password !== "string" ||
    typeof wifi.security !== "string" ||
    typeof wifi.hidden !== "boolean"
  ) {
    return null;
  }
  if (!wifi.ssid || byteLength(wifi.ssid) > MAX_WIFI_SSID_BYTES) return null;
  if (byteLength(wifi.password) > MAX_WIFI_PASSWORD_BYTES) return null;
  if (!["WPA", "WEP", "nopass"].includes(wifi.security)) return null;
  if (wifi.security === "nopass" ? wifi.password.length > 0 : wifi.password.length === 0) return null;
  const payload = `WIFI:T:${wifi.security};S:${escapeWifi(wifi.ssid)};P:${escapeWifi(wifi.password)}${wifi.hidden ? ";H:true" : ""};;`;
  return byteLength(payload) <= MAX_PAYLOAD_BYTES ? payload : null;
}

function escapeWifi(value: string): string {
  return [...value].map((character) => /[\\;,:]/u.test(character) ? `\\${character}` : character).join("");
}

function encodeUtf8(value: string): Uint8Array | null {
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code >= 0xd800 && code <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (index + 1 >= value.length || next < 0xdc00 || next > 0xdfff) return null;
      index += 1;
    } else if (code >= 0xdc00 && code <= 0xdfff) {
      return null;
    }
  }
  return new TextEncoder().encode(value);
}

function byteLength(value: string): number {
  const bytes = encodeUtf8(value);
  return bytes?.length ?? Number.POSITIVE_INFINITY;
}

function renderSvg(
  code: ReturnType<typeof qrcode>,
  scale: number,
  quietZone: number,
  width: number,
): string {
  const modules = code.getModuleCount();
  let svg = `<?xml version="1.0" encoding="UTF-8"?><svg xmlns="http://www.w3.org/2000/svg" version="1.1" width="${width}" height="${width}" viewBox="0 0 ${width} ${width}" shape-rendering="crispEdges"><rect width="${width}" height="${width}" fill="#fff"/><path fill="#000" d="`;
  for (let row = 0; row < modules; row += 1) {
    let column = 0;
    while (column < modules) {
      if (!code.isDark(row, column)) {
        column += 1;
        continue;
      }
      const start = column;
      while (column < modules && code.isDark(row, column)) column += 1;
      const left = (start + quietZone) * scale;
      const top = (row + quietZone) * scale;
      const run = (column - start) * scale;
      svg += `M${left} ${top}h${run}v${scale}H${left}V${top}`;
    }
  }
  svg += `"/></svg>`;
  if (new TextEncoder().encode(svg).length > MAX_SVG_OUTPUT_BYTES) {
    throw new QrGenerationError("render");
  }
  return svg;
}

function renderPng(
  code: ReturnType<typeof qrcode>,
  scale: number,
  quietZone: number,
  width: number,
): string {
  if (typeof document === "undefined") throw new QrGenerationError("render");
  try {
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = width;
    const context = canvas.getContext("2d");
    if (!context) throw new QrGenerationError("render");
    context.fillStyle = "#fff";
    context.fillRect(0, 0, width, width);
    context.fillStyle = "#000";
    const modules = code.getModuleCount();
    for (let row = 0; row < modules; row += 1) {
      for (let column = 0; column < modules; column += 1) {
        if (code.isDark(row, column)) {
          context.fillRect((column + quietZone) * scale, (row + quietZone) * scale, scale, scale);
        }
      }
    }
    const dataUrl = canvas.toDataURL("image/png");
    const prefix = "data:image/png;base64,";
    if (!dataUrl.startsWith(prefix)) throw new QrGenerationError("render");
    const base64 = dataUrl.slice(prefix.length);
    if (base64.length > MAX_BINARY_OUTPUT_BASE64_LENGTH) throw new QrGenerationError("render");
    return base64;
  } catch (error) {
    if (error instanceof QrGenerationError) throw error;
    throw new QrGenerationError("render");
  }
}
