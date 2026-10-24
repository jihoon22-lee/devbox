import { useEffect, useRef, useState } from "react";
export function downloadTextResult(value: string, filename: string): void {
  const url = URL.createObjectURL(new Blob([value], { type: "text/plain;charset=utf-8" }));
  try {
    const a = document.createElement("a");
    a.href = url;
    a.download = filename;
    a.click();
  } finally {
    URL.revokeObjectURL(url);
  }
}
export function downloadBinaryResult(value: string, filename: string, mimeType: string): void {
  const bytes = Uint8Array.from(atob(value), (char) => char.charCodeAt(0));
  const url = URL.createObjectURL(new Blob([bytes], { type: mimeType }));
  try {
    const a = document.createElement("a");
    a.href = url;
    a.download = filename;
    a.click();
  } finally {
    URL.revokeObjectURL(url);
  }
}
export function CopyBtn({ value }: { value: string }) {
  const [result, setResult] = useState<{ value: string; failed: boolean } | null>(null);
  const currentValue = useRef(value);
  currentValue.current = value;
  const revision = useRef(0);
  useEffect(
    () => () => {
      revision.current += 1;
    },
    [],
  );
  const copy = async () => {
    const current = ++revision.current;
    setResult(null);
    try {
      await navigator.clipboard.writeText(value);
    } catch {
      if (current === revision.current && currentValue.current === value) setResult({ value, failed: true });
    }
  };
  if (!value) return null;
  return (
    <>
      <button type="button" className="copy-btn" onClick={() => void copy()}>
        복사
      </button>
      {result?.value === value && result.failed && <span role="alert">결과를 클립보드에 복사하지 못했습니다.</span>}
    </>
  );
}
