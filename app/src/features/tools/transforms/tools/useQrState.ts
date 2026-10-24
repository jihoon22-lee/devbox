import { useEffect, useRef, useState } from "react";
import { downloadBinaryResult, downloadTextResult } from "../frame";
import { generateQr } from "../lib/browser";
import {
  type GenerateQrRequest,
  QR_ERROR_MESSAGES,
  type QrErrorCorrection,
  QrGenerationError,
  type QrPreset,
  type QrResult,
  type QrVersion,
  type WifiRequest,
} from "../lib/qr";
import { EMPTY_WIFI, FIXED_ACTION_ERROR } from "./qrConfiguration";
export function useQrState() {
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
  const actionId = useRef(0);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      requestId.current += 1;
      actionId.current += 1;
      mounted.current = false;
    };
  }, []);

  const invalidate = (change: () => void) => {
    if (runningRef.current) return;
    requestId.current += 1;
    actionId.current += 1;
    runningRef.current = false;
    setRunning(false);
    setResult(null);
    setError(null);
    setActionError(null);
    change();
  };

  const hasPayload = preset === "text" ? text.length > 0 : preset === "url" ? url.length > 0 : wifi.ssid.length > 0;

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
      version: versionValue === null ? null : (versionValue as QrVersion),
      errorCorrection,
      size,
      quietZone,
    };
    const currentRequest = ++requestId.current;
    actionId.current += 1;
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
    const currentAction = ++actionId.current;
    if (!result || !navigator.clipboard?.writeText) {
      setActionError(FIXED_ACTION_ERROR);
      return;
    }
    try {
      void navigator.clipboard
        .writeText(result.svg)
        .then(() => {
          if (mounted.current && actionId.current === currentAction) setActionError(null);
        })
        .catch(() => {
          if (mounted.current && actionId.current === currentAction) setActionError(FIXED_ACTION_ERROR);
        });
    } catch {
      setActionError(FIXED_ACTION_ERROR);
    }
  };

  const copyPng = () => {
    const currentAction = ++actionId.current;
    if (!result || typeof ClipboardItem === "undefined" || !navigator.clipboard?.write) {
      setActionError("이 환경에서는 PNG를 클립보드에 복사할 수 없습니다. PNG 저장을 사용하세요.");
      return;
    }
    try {
      const bytes = decodeBase64(result.pngBase64);
      const item = new ClipboardItem({ "image/png": new Blob([bytes], { type: "image/png" }) });
      void navigator.clipboard
        .write([item])
        .then(() => {
          if (mounted.current && actionId.current === currentAction) setActionError(null);
        })
        .catch(() => {
          if (mounted.current && actionId.current === currentAction) {
            setActionError("PNG를 클립보드에 복사할 수 없습니다. PNG 저장을 사용하세요.");
          }
        });
    } catch {
      setActionError(FIXED_ACTION_ERROR);
    }
  };

  const saveSvg = () => {
    actionId.current += 1;
    if (!result) return;
    try {
      downloadTextResult(result.svg, "devbox-qr.svg");
      setActionError(null);
    } catch {
      setActionError(FIXED_ACTION_ERROR);
    }
  };

  const savePng = () => {
    actionId.current += 1;
    if (!result) return;
    try {
      downloadBinaryResult(result.pngBase64, "devbox-qr.png", "image/png");
      setActionError(null);
    } catch {
      setActionError(FIXED_ACTION_ERROR);
    }
  };

  return {
    preset,
    setPreset,
    text,
    setText,
    url,
    setUrl,
    wifi,
    setWifiField,
    versionText,
    setVersionText,
    errorCorrection,
    setErrorCorrection,
    sizeText,
    setSizeText,
    quietZoneText,
    setQuietZoneText,
    result,
    error,
    actionError,
    running,
    composing,
    invalidate,
    hasPayload,
    run,
    copyPng,
    savePng,
    copySvg,
    saveSvg,
  };
}

function decodeBase64(value: string): Uint8Array {
  const binary = atob(value);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
  return bytes;
}

export type QrState = ReturnType<typeof useQrState>;
