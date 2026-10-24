/** Local browser transformation inputs/results. No IPC or persistence. */
export type HmacRequest = {
  algorithm: string;
  key: string;
  keyEncoding: string;
  message: string;
  messageEncoding: string;
  outputEncoding: string;
};

export type HmacVerifyRequest = {
  algorithm: string;
  key: string;
  keyEncoding: string;
  message: string;
  messageEncoding: string;
  outputEncoding: string;
  expectedTag: string;
};

export type WifiRequest = { ssid: string; password: string; security: string; hidden: boolean };

export type GenerateQrRequest = {
  preset: string;
  text?: string | null;
  url?: string | null;
  wifi?: WifiRequest | null;
  /**
   * `None` selects the smallest normal QR version that fits.
   */
  version?: number | null;
  errorCorrection: string;
  /**
   * Maximum requested edge length in pixels. The actual edge is module-aligned.
   */
  size: number;
  quietZone: number;
};

export type QrResult = {
  svg: string;
  pngBase64: string;
  width: number;
  version: number;
  modules: number;
  quietZone: number;
  payloadBytes: number;
};

export type JwtVerifyRequest = {
  algorithm: string;
  signingInput: string;
  signature: string;
  key: string;
  keyEncoding: string;
};

export type DiffHunk = {
  /**
   * 0 = 같은 구간, 1 = 삽입(+), 2 = 삭제(-)
   */
  kind: number;
  old_start: number;
  old_end: number;
  new_start: number;
  new_end: number;
};
