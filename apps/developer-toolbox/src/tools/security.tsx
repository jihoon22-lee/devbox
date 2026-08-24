import { useState } from "react";
import { generateUuid, hash } from "../api";
import { CopyBtn, ToolOutput, ToolTextArea } from "./common";

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

export function UuidTool() {
  const [count, setCount] = useState(5);
  const [list, setList] = useState<string[]>([]);

  const run = async () => {
    const next: string[] = [];
    for (let i = 0; i < count; i++) next.push(await generateUuid());
    setList(next);
  };

  return (
    <div className="tool">
      <div className="row">
        <label>
          Count
          <input
            type="number"
            min={1}
            max={100}
            value={count}
            onChange={(e) => setCount(Number(e.currentTarget.value))}
          />
        </label>
        <button className="btn" onClick={() => void run()}>
          Generate
        </button>
      </div>
      <ToolOutput
        className="uuid-list"
        value={list.join("\n")}
        ariaLabel="UUID output"
        downloadName="dev-toolbox-uuid-result.txt"
      />
      {list.length > 0 && <CopyBtn value={list.join("\n")} />}
    </div>
  );
}
