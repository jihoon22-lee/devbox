import { useEffect, useState } from "react";
import { diff } from "../api";
import type { DiffHunk } from "../types";
import { ToolOutput, ToolTextArea } from "./common";

const KIND_CLASS = ["diff-eq", "diff-add", "diff-del"];

/** 라인 단위 diff를 두 컬럼으로 표시하는 도구 */
export function DiffTool() {
  const [a, setA] = useState("");
  const [b, setB] = useState("");
  const [hunks, setHunks] = useState<DiffHunk[]>([]);

  useEffect(() => {
    let cancelled = false;
    diff(a, b).then((h) => {
      if (!cancelled) setHunks(h);
    });
    return () => {
      cancelled = true;
    };
  }, [a, b]);

  const aLines = a.split("\n");
  const bLines = b.split("\n");

  const renderColumn = (lines: string[], side: "old" | "new") => {
    return hunks.map((h, i) => {
      const start = side === "old" ? h.old_start : h.new_start;
      const end = side === "old" ? h.old_end : h.new_end;
      if (h.kind === 0) {
        return lines.slice(start, end).map((ln, j) => (
          <div key={`${i}-${j}`} className="diff-line diff-eq">
            {ln || " "}
          </div>
        ));
      }
      // kind 1(insert)은 new에만, kind 2(delete)는 old에만 해당 라인이 실제로 있음
      if (side === "old") {
        if (h.kind === 2) {
          return lines.slice(start, end).map((ln, j) => (
            <div key={`${i}-${j}`} className="diff-line diff-del">
              {ln || " "}
            </div>
          ));
        }
        return Array.from({ length: end - start }, (_, j) => (
          <div key={`${i}-${j}`} className="diff-line diff-gap" />
        ));
      }
      // new side
      if (h.kind === 1) {
        return lines.slice(start, end).map((ln, j) => (
          <div key={`${i}-${j}`} className="diff-line diff-add">
            {ln || " "}
          </div>
        ));
      }
      return Array.from({ length: end - start }, (_, j) => (
        <div key={`${i}-${j}`} className="diff-line diff-gap" />
      ));
    });
  };

  void KIND_CLASS;

  return (
    <div className="tool">
      <div className="io-grid">
        <div className="io-col">
          <div className="io-label">Old</div>
          <ToolTextArea
            aria-label="Old input"
            className="io-input"
            rows={10}
            value={a}
            onValueChange={setA}
            spellCheck={false}
          />
        </div>
        <div className="io-col">
          <div className="io-label">New</div>
          <ToolTextArea
            aria-label="New input"
            className="io-input"
            rows={10}
            value={b}
            onValueChange={setB}
            spellCheck={false}
          />
        </div>
      </div>
      <div className="io-grid diff-view">
        <ToolOutput
          asDiv
          className="io-col"
          value={a}
          ariaLabel="Old diff output"
          downloadName="dev-toolbox-diff-old.txt"
        >
          {renderColumn(aLines, "old")}
        </ToolOutput>
        <ToolOutput
          asDiv
          className="io-col"
          value={b}
          ariaLabel="New diff output"
          downloadName="dev-toolbox-diff-new.txt"
        >
          {renderColumn(bLines, "new")}
        </ToolOutput>
      </div>
    </div>
  );
}
