import type { QrErrorCorrection, QrPreset, WifiRequest } from "../lib/qr";
export const ERROR_CORRECTION_LEVELS: ReadonlyArray<{ value: QrErrorCorrection; label: string }> = [
  { value: "L", label: "L · 낮음" },
  { value: "M", label: "M · 중간" },
  { value: "Q", label: "Q · 높음" },
  { value: "H", label: "H · 최고" },
];

export const PRESETS: ReadonlyArray<{ value: QrPreset; label: string }> = [
  { value: "text", label: "텍스트" },
  { value: "url", label: "URL" },
  { value: "wifi", label: "Wi-Fi" },
];

export const VERSION_OPTIONS: ReadonlyArray<{ value: string; label: string }> = [
  { value: "auto", label: "자동 (최소 버전)" },
  ...Array.from({ length: 40 }, (_, index) => {
    const version = index + 1;
    return { value: String(version), label: `버전 ${version}` };
  }),
];

export const FIXED_ACTION_ERROR = "QR 작업을 완료하지 못했습니다.";

export const EMPTY_WIFI: WifiRequest = {
  ssid: "",
  password: "",
  security: "WPA",
  hidden: false,
};
