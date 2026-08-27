import { useCallback, useEffect, useRef, useState } from "react";
import { downloadTextResult, ToolOutput, ToolTextField } from "./common";
import {
  generateLorem,
  LOREM_ERROR_MESSAGES,
  MAX_LOREM_COUNT,
  MAX_LOREM_COUNT_DIGITS,
  MAX_LOREM_COUNT_INPUT_BYTES,
  MAX_LOREM_OUTPUT_BYTES,
  parseLoremCount,
  type LoremUnit,
} from "./lorem";

const UNIT_OPTIONS: ReadonlyArray<{ value: LoremUnit; label: string; countLabel: string }> = [
  { value: "paragraphs", label: "문단", countLabel: "문단" },
  { value: "sentences", label: "문장", countLabel: "문장" },
  { value: "words", label: "단어", countLabel: "단어" },
];

const OUTPUT_FILENAME = "lorem-ipsum.txt";

function unitLabel(unit: LoremUnit): string {
  return UNIT_OPTIONS.find((option) => option.value === unit)?.countLabel ?? "단위";
}

const FIXED_COPY_ERROR = "Lorem 결과를 clipboard에 복사하지 못했습니다.";
const FIXED_SAVE_ERROR = "Lorem 결과 파일을 저장하지 못했습니다.";
const FIXED_CONTEXT_ERROR = "Lorem 결과 작업을 완료하지 못했습니다.";
const FIXED_INPUT_ERROR = "Lorem 입력을 붙여넣지 못했습니다.";

/** Offline Lorem generator UI; generation itself is synchronous and therefore cannot overlap. */
export function LoremTool() {
  const [unit, setUnit] = useState<LoremUnit>("paragraphs");
  const [countText, setCountText] = useState("3");
  const [output, setOutput] = useState("");
  const [generatedCount, setGeneratedCount] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [actionBusy, setActionBusy] = useState(false);
  const actionBusyRef = useRef(false);
  const actionRevision = useRef(0);
  const mounted = useRef(true);
  const composing = useRef(false);
  const [isComposing, setIsComposing] = useState(false);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      actionRevision.current += 1;
    };
  }, []);

  const count = parseLoremCount(countText);
  const validCount = count !== null;

  const clearOutput = useCallback(() => {
    actionRevision.current += 1;
    actionBusyRef.current = false;
    setActionBusy(false);
    setOutput("");
    setGeneratedCount(null);
    setError(null);
    setActionError(null);
  }, []);

  const onOutputBusyChange = useCallback((busy: boolean) => {
    actionBusyRef.current = busy;
    if (busy) setActionError(null);
    setActionBusy(busy);
  }, []);

  const run = () => {
    if (composing.current || actionBusyRef.current) return;
    if (count === null) {
      setError(LOREM_ERROR_MESSAGES.INVALID_COUNT);
      setOutput("");
      setGeneratedCount(null);
      return;
    }

    const result = generateLorem({ unit, count });
    if (result.error) {
      setError(result.error.message);
      setOutput("");
      setGeneratedCount(null);
      return;
    }
    setError(null);
    setOutput(result.output);
    setGeneratedCount(result.unitCount);
    setActionError(null);
  };

  const copy = () => {
    if (!output || actionBusyRef.current) return;
    const snapshot = output;
    const revision = ++actionRevision.current;
    actionBusyRef.current = true;
    setActionBusy(true);
    setActionError(null);
    void Promise.resolve()
      .then(() => {
        if (!navigator.clipboard) throw new Error("clipboard unavailable");
        return navigator.clipboard.writeText(snapshot);
      })
      .then(() => {
        if (mounted.current && actionRevision.current === revision) setActionError(null);
      })
      .catch(() => {
        if (mounted.current && actionRevision.current === revision) setActionError(FIXED_COPY_ERROR);
      })
      .finally(() => {
        if (mounted.current && actionRevision.current === revision) {
          actionBusyRef.current = false;
          setActionBusy(false);
        }
      });
  };

  const save = () => {
    if (!output || actionBusyRef.current) return;
    const snapshot = output;
    const revision = ++actionRevision.current;
    actionBusyRef.current = true;
    setActionBusy(true);
    setActionError(null);
    try {
      downloadTextResult(snapshot, OUTPUT_FILENAME);
      if (mounted.current && actionRevision.current === revision) setActionError(null);
    } catch {
      if (mounted.current && actionRevision.current === revision) setActionError(FIXED_SAVE_ERROR);
    } finally {
      if (mounted.current && actionRevision.current === revision) {
        actionBusyRef.current = false;
        setActionBusy(false);
      }
    }
  };

  return (
    <div className="tool lorem-tool" aria-busy={actionBusy}>
      <div className="lorem-toolbar">
        <label className="lorem-field">
          분량 단위
          <select
            aria-label="Lorem 분량 단위"
            value={unit}
            disabled={actionBusy}
            onChange={(event) => {
              clearOutput();
              setUnit(event.currentTarget.value as LoremUnit);
            }}
          >
            {UNIT_OPTIONS.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </label>
        <label className="lorem-field">
          수량
          <ToolTextField
            aria-label="Lorem 수량"
            aria-describedby={!validCount && countText.trim() !== "" ? "lorem-help lorem-count-error" : "lorem-help"}
            aria-invalid={!validCount && countText.trim() !== ""}
            className="lorem-count"
            inputMode="numeric"
            maxLength={MAX_LOREM_COUNT_DIGITS}
            maxPasteBytes={MAX_LOREM_COUNT_INPUT_BYTES}
            clipboardErrorMessage={FIXED_INPUT_ERROR}
            value={countText}
            disabled={actionBusy}
            onCompositionStart={() => {
              composing.current = true;
              setIsComposing(true);
            }}
            onCompositionEnd={() => {
              composing.current = false;
              setIsComposing(false);
            }}
            onValueChange={(value) => {
              clearOutput();
              setCountText(value);
            }}
            spellCheck={false}
          />
        </label>
        <button
          type="button"
          className="btn"
          disabled={!validCount || actionBusy || isComposing}
          onClick={run}
        >
          생성
        </button>
      </div>

      <div id="lorem-help" className="lorem-help" role="note">
        고정된 로컬 corpus로 같은 단위·수량에서 항상 같은 결과를 만듭니다. 네트워크 요청,
        자동 저장, 입력 수집은 없으며 결과는 명시적으로 복사하거나 파일로 저장할 때만 외부로
        나갑니다. 수량은 1–{MAX_LOREM_COUNT}, 결과는 최대 {MAX_LOREM_OUTPUT_BYTES.toLocaleString()}바이트입니다.
      </div>

      {!validCount && countText.trim() !== "" ? (
        <div id="lorem-count-error" className="lorem-error" role="alert">
          {LOREM_ERROR_MESSAGES.INVALID_COUNT}
        </div>
      ) : null}
      {error ? <div className="lorem-error" role="alert">{error}</div> : null}
      <div className="lorem-status" role="status" aria-live="polite" aria-atomic="true">
        {generatedCount === null ? "" : `${generatedCount}개 ${unitLabel(unit)}을 생성했습니다.`}
      </div>

      <div className="lorem-output-toolbar">
        <span className="io-label">출력</span>
        <span className="conversion-actions">
          <button type="button" className="copy-btn" disabled={!output || actionBusy} onClick={copy}>
            복사
          </button>
          <button type="button" className="copy-btn" disabled={!output || actionBusy} onClick={save}>
            저장
          </button>
        </span>
      </div>
      <ToolOutput
        ariaLabel="Lorem 출력"
        actionErrorMessage={FIXED_CONTEXT_ERROR}
        busy={actionBusy}
        onBusyChange={onOutputBusyChange}
        className="io-output lorem-output"
        value={output}
        downloadName={OUTPUT_FILENAME}
      />
      {actionError ? <div className="context-action-error" role="alert">{actionError}</div> : null}
    </div>
  );
}
