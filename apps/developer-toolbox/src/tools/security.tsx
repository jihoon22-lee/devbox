import { useEffect, useRef, useState } from "react";
import { generateIds, hash } from "../api";
import { CopyBtn, ToolOutput, ToolTextArea } from "./common";
import {
  IDENTIFIER_GENERATION_ERROR,
  MAX_IDENTIFIER_BATCH,
  type IdentifierKind,
} from "./ids";

const ALGORITHMS = ["md5", "sha256", "sha512"];

export function HashTool() {
  const [input, setInput] = useState("");
  const [algorithm, setAlgorithm] = useState("sha256");
  const [output, setOutput] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);

  const run = async () => {
    setRunning(true);
    setError(null);
    try {
      setOutput(await hash(input, algorithm));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setOutput("");
    } finally {
      setRunning(false);
    }
  };

  return (
    <div className="tool">
      <div className="row">
        <select value={algorithm} onChange={(e) => setAlgorithm(e.currentTarget.value)}>
          {ALGORITHMS.map((a) => (
            <option key={a} value={a}>
              {a}
            </option>
          ))}
        </select>
        <button className="btn" onClick={() => void run()} disabled={running || !input}>
          Hash
        </button>
      </div>
      <div className="io-grid">
        <div className="io-col">
          <div className="io-label">Input</div>
          <ToolTextArea
            aria-label="Hash input"
            className="io-input"
            rows={5}
            value={input}
            onValueChange={setInput}
            spellCheck={false}
          />
        </div>
        <div className="io-col">
          <div className="io-label">
            Output {output && <CopyBtn value={output} />}
          </div>
          <ToolOutput
            className={`io-output ${error ? "io-error" : ""}`}
            value={error || output}
            downloadName="dev-toolbox-hash-result.txt"
          />
        </div>
      </div>
    </div>
  );
}

const IDENTIFIER_KINDS: ReadonlyArray<{ value: IdentifierKind; label: string }> = [
  { value: "uuid-v4", label: "UUID v4" },
  { value: "uuid-v7", label: "UUID v7" },
  { value: "ulid", label: "ULID" },
];

export function UuidTool() {
  const [kind, setKind] = useState<IdentifierKind>("uuid-v4");
  const [countText, setCountText] = useState("5");
  const [uppercase, setUppercase] = useState(false);
  const [hyphens, setHyphens] = useState(true);
  const [list, setList] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const requestId = useRef(0);
  const runningRef = useRef(false);
  const composing = useRef(false);

  useEffect(() => {
    // A late IPC/browser promise must not update a component that has left the
    // tool view. Incrementing the same request sequence used for stale-result
    // protection keeps unmount cleanup side-effect free.
    return () => {
      requestId.current += 1;
    };
  }, []);

  const count = Number(countText);
  const validCount =
    countText.trim() !== "" &&
    Number.isInteger(count) &&
    count >= 1 &&
    count <= MAX_IDENTIFIER_BATCH;

  const invalidatePending = () => {
    requestId.current += 1;
    runningRef.current = false;
    setRunning(false);
    setList([]);
    setError(null);
  };

  const changeKind = (next: IdentifierKind) => {
    invalidatePending();
    setKind(next);
    // UUID keeps the previous tool's lower-case canonical form. ULID's
    // canonical representation is upper-case and hyphenless.
    setUppercase(next === "ulid");
    setHyphens(next !== "ulid");
  };

  const changeOptions = (change: () => void) => {
    invalidatePending();
    change();
  };

  const run = async () => {
    // Do not submit a partially composed numeric value if an IME is active.
    if (composing.current || runningRef.current) return;
    if (!validCount) {
      setError(`생성 수량은 1에서 ${MAX_IDENTIFIER_BATCH} 사이여야 합니다.`);
      setList([]);
      return;
    }
    const currentRequest = ++requestId.current;
    runningRef.current = true;
    setRunning(true);
    setError(null);
    try {
      const generated = await generateIds({ kind, count, uppercase, hyphens });
      if (requestId.current !== currentRequest) return;
      setList(generated);
    } catch {
      if (requestId.current !== currentRequest) return;
      setError(IDENTIFIER_GENERATION_ERROR);
      setList([]);
    } finally {
      if (requestId.current === currentRequest) {
        runningRef.current = false;
        setRunning(false);
      }
    }
  };

  return (
    <div className="tool id-generator-tool" aria-busy={running}>
      <div className="id-generator-toolbar">
        <label className="id-generator-field">
          식별자
          <select
            id="identifier-kind"
            aria-label="식별자 종류"
            aria-describedby="identifier-help"
            value={kind}
            disabled={running}
            onChange={(event) => changeKind(event.currentTarget.value as IdentifierKind)}
          >
            {IDENTIFIER_KINDS.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </label>
        <label className="id-generator-field">
          수량
          <input
            id="identifier-count"
            type="number"
            min={1}
            max={MAX_IDENTIFIER_BATCH}
            step={1}
            inputMode="numeric"
            aria-label="생성 수량"
            aria-describedby={
              !validCount && countText.trim() !== ""
                ? "identifier-count-error identifier-help"
                : "identifier-help"
            }
            aria-invalid={!validCount && countText.trim() !== ""}
            value={countText}
            disabled={running}
            onCompositionStart={() => {
              composing.current = true;
            }}
            onCompositionEnd={() => {
              composing.current = false;
            }}
            onChange={(event) =>
              changeOptions(() => setCountText(event.currentTarget.value))
            }
          />
        </label>
        <label className="id-generator-check">
          <input
            id="identifier-uppercase"
            type="checkbox"
            checked={uppercase}
            aria-label="대문자 출력"
            aria-describedby="identifier-help"
            disabled={running}
            onChange={(event) =>
              changeOptions(() => setUppercase(event.currentTarget.checked))
            }
          />
          <span>대문자</span>
        </label>
        <label className="id-generator-check">
          <input
            id="identifier-hyphens"
            type="checkbox"
            checked={hyphens}
            aria-label="하이픈 표시"
            aria-describedby="identifier-help"
            disabled={running}
            onChange={(event) =>
              changeOptions(() => setHyphens(event.currentTarget.checked))
            }
          />
          <span>하이픈 표시</span>
        </label>
        <button
          type="button"
          className="btn"
          onClick={() => void run()}
          disabled={running || !validCount}
        >
          {running ? "생성 중..." : "생성"}
        </button>
      </div>
      <div id="identifier-help" className="id-generator-help" role="note">
        {kind === "ulid"
          ? "ULID는 timestamp와 난수로 구성된 Crockford Base32 식별자입니다. 기본 출력은 canonical 대문자·하이픈 없음이며, 하이픈은 표시용 그룹입니다. 한 batch 안에서는 생성 순서대로 증가하고, 별도 호출·프로세스 간 전역 순서는 보장하지 않습니다."
          : "UUID v4는 난수 기반이며 순서를 보장하지 않습니다. UUID v7은 현재 시각과 난수 기반으로 한 batch 안에서 생성 순서대로 증가합니다. 별도 호출·프로세스 간 전역 순서는 보장하지 않습니다. 입력·결과는 자동 저장하거나 전송하지 않습니다."}
      </div>
      {!validCount && countText.trim() !== "" ? (
        <div id="identifier-count-error" className="id-generator-error" role="alert">
          생성 수량은 1에서 {MAX_IDENTIFIER_BATCH} 사이의 정수여야 합니다.
        </div>
      ) : null}
      {error ? <div className="id-generator-error" role="alert">{error}</div> : null}
      <div className="id-generator-status" role="status" aria-live="polite" aria-atomic="true">
        {running
          ? "식별자를 생성하는 중입니다."
          : list.length > 0
            ? `${list.length}개 식별자를 생성했습니다.`
            : ""}
      </div>
      <ToolOutput
        className="uuid-list"
        value={list.join("\n")}
        ariaLabel="생성된 식별자 출력"
        downloadName="dev-toolbox-identifiers.txt"
      />
      {list.length > 0 ? <CopyBtn value={list.join("\n")} /> : null}
    </div>
  );
}
