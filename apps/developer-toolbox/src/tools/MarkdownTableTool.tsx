import { useCallback, useEffect, useRef, useState } from "react";
import { downloadTextResult, ToolOutput, ToolTextArea } from "./common";
import {
  formatMarkdownTable,
  MARKDOWN_TABLE_LIMITS,
  type MarkdownTableError,
  type MarkdownTableResult,
} from "./markdownTable";

interface TransformState extends MarkdownTableResult {
  input: string;
  running: boolean;
}

const INITIAL_STATE: TransformState = {
  input: "",
  output: "",
  error: null,
  running: false,
};

const FIXED_COPY_ERROR = "변환 결과를 clipboard에 복사하지 못했습니다.";
const FIXED_SAVE_ERROR = "변환 결과 파일을 저장하지 못했습니다.";
const FIXED_CONTEXT_ERROR = "변환 결과 작업을 완료하지 못했습니다.";
const FIXED_INPUT_ERROR = "표 입력을 붙여넣지 못했습니다.";

function failedResult(): MarkdownTableResult {
  return {
    output: "",
    error: {
      code: "FORMAT_FAILED",
      message: "변환을 완료하지 못했습니다.",
    },
  };
}

/**
 * Queue the bounded formatter after a rendering opportunity. Superseded queued
 * work is cancelled; the sequence check remains the authoritative guard once
 * the synchronous bounded core has started.
 */
function useMarkdownTableTransform(input: string): TransformState {
  const [state, setState] = useState<TransformState>(INITIAL_STATE);
  const requestSequence = useRef(0);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      requestSequence.current += 1;
    };
  }, []);

  useEffect(() => {
    const current = ++requestSequence.current;
    const controller = typeof AbortController === "function" ? new AbortController() : null;
    setState({ input, output: "", error: null, running: true });

    const timer = setTimeout(() => {
      if (controller?.signal.aborted || !mounted.current || requestSequence.current !== current) {
        return;
      }
      try {
        const result = formatMarkdownTable(input);
        if (controller?.signal.aborted || !mounted.current || requestSequence.current !== current) {
          return;
        }
        setState({ ...result, input, running: false });
      } catch {
        if (controller?.signal.aborted || !mounted.current || requestSequence.current !== current) {
          return;
        }
        setState({ ...failedResult(), input, running: false });
      }
    }, 0);

    return () => {
      clearTimeout(timer);
      controller?.abort();
      if (requestSequence.current === current) requestSequence.current += 1;
    };
  }, [input]);

  return state;
}

function MarkdownTableErrorView({ error }: { error: MarkdownTableError }) {
  return (
    <div className="conversion-error" role="alert">
      <strong>{error.message}</strong>
      <span>{error.code}</span>
    </div>
  );
}

export function MarkdownTableTool() {
  const [input, setInput] = useState("");
  const resolved = useMarkdownTableTransform(input);
  const [actionError, setActionError] = useState<string | null>(null);
  const [actionBusy, setActionBusy] = useState(false);
  const actionBusyRef = useRef(false);
  const actionRevision = useRef(0);
  const mounted = useRef(true);
  const current = resolved.input === input;
  const result = current
    ? resolved
    : { ...INITIAL_STATE, input, running: true };
  const canAct = current && !result.running && !result.error && result.output.length > 0;

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      actionRevision.current += 1;
    };
  }, []);

  useEffect(() => {
    actionRevision.current += 1;
    actionBusyRef.current = false;
    setActionBusy(false);
    setActionError(null);
  }, [input, result.error?.code, result.output]);

  const onOutputBusyChange = useCallback((busy: boolean) => {
    actionBusyRef.current = busy;
    if (busy) setActionError(null);
    setActionBusy(busy);
  }, []);

  const handleInputChange = useCallback((value: string) => {
    // Invalidate output actions synchronously, before the formatter effect or
    // a pending clipboard promise can observe the next render.
    actionRevision.current += 1;
    actionBusyRef.current = false;
    setActionBusy(false);
    setActionError(null);
    setInput(value);
  }, []);

  const copy = () => {
    if (!canAct || actionBusyRef.current) return;
    const snapshot = result.output;
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
    if (!canAct || actionBusyRef.current) return;
    const snapshot = result.output;
    const revision = ++actionRevision.current;
    actionBusyRef.current = true;
    setActionBusy(true);
    setActionError(null);
    try {
      downloadTextResult(snapshot, "formatted-table.md");
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
    <div className="tool markdown-table-tool" aria-busy={actionBusy}>
      <div className="conversion-notice" role="note">
        <strong>사용법</strong>
        <span>
          파이프(|)로 구분한 표를 붙여 넣으세요. 두 번째 행에 <code>---</code>, <code>:---</code>,
          <code>---:</code>, <code>:---:</code>를 쓰면 열 정렬을 유지합니다. 열이 부족한 행은 빈 셀로
          채우고, 누락된 구분 행은 자동으로 추가합니다. 짝이 맞는 백틱 코드 구간 안의 파이프는
          셀 내용으로 유지하며 원본 행·열 순서는 바꾸지 않습니다.
        </span>
      </div>

      <div className="io-grid">
        <div className="io-col">
          <div className="io-label">입력 · Markdown 표</div>
          <ToolTextArea
            aria-label="Markdown 표 입력"
            aria-describedby="markdown-table-help"
            className="io-input markdown-table-input"
            placeholder={'| 이름 | 값 |\n| --- | --- |\n| devbox | 0.5 |'}
            rows={18}
            value={input}
            onValueChange={handleInputChange}
            maxPasteBytes={MARKDOWN_TABLE_LIMITS.maxInputBytes}
            clipboardErrorMessage={FIXED_INPUT_ERROR}
            spellCheck={false}
          />
          <span id="markdown-table-help" className="markdown-table-help">
            최대 {MARKDOWN_TABLE_LIMITS.maxInputBytes.toLocaleString()}바이트 · {MARKDOWN_TABLE_LIMITS.maxRows}행 · {MARKDOWN_TABLE_LIMITS.maxColumns}열 · 셀 {MARKDOWN_TABLE_LIMITS.maxCellCodePoints.toLocaleString()}자
          </span>
        </div>

        <div className="io-col" aria-busy={result.running}>
          <div className="io-label conversion-output-label">
            출력 · 정렬된 Markdown
            {result.running ? (
              <span className="dim" role="status" aria-live="polite" aria-atomic="true">
                (변환 중...)
              </span>
            ) : null}
            <span className="conversion-actions">
              <button type="button" className="copy-btn" disabled={!canAct || actionBusy} onClick={copy}>
                복사
              </button>
              <button type="button" className="copy-btn" disabled={!canAct || actionBusy} onClick={save}>
                저장
              </button>
            </span>
          </div>
          {result.error ? <MarkdownTableErrorView error={result.error} /> : null}
          <ToolOutput
            ariaLabel="Markdown 표 출력"
            actionErrorMessage={FIXED_CONTEXT_ERROR}
            busy={actionBusy}
            onBusyChange={onOutputBusyChange}
            className="io-output markdown-table-output"
            value={canAct ? result.output : ""}
            downloadName="formatted-table.md"
          />
          {actionError ? <div className="context-action-error" role="alert">{actionError}</div> : null}
        </div>
      </div>
    </div>
  );
}
