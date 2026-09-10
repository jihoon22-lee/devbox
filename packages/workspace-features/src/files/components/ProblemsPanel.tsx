// 열린 문서의 LSP 진단을 한 곳에 모은 Problems panel (§12.2).
// error/warning 필터, 클릭 이동. stale diagnostics는 보여주지 않는다
// (부모가 항상 최신 lspDiagnostics만 넘긴다).

import { useMemo, useState } from "react";
import type { Diagnostic } from "@codemirror/lint";

export interface ProblemDoc {
  id: string;
  path: string;
}

interface Props {
  docs: ProblemDoc[];
  /** 문서 id → 진단 목록 (최신만). */
  diagnosticsFor: (docId: string) => Diagnostic[];
  /** LSP 서버 상태 요약 (degraded/crash 표시용). */
  serverStatus: string;
  onNavigate: (docId: string, offset: number) => void;
  onClose: () => void;
}

type SeverityFilter = "all" | "error" | "warning";

export default function ProblemsPanel({ docs, diagnosticsFor, serverStatus, onNavigate, onClose }: Props) {
  const [severity, setSeverity] = useState<SeverityFilter>("all");
  const [sourceFilter, setSourceFilter] = useState("");

  const rows = useMemo(() => {
    const out: Array<{ docId: string; path: string; diag: Diagnostic }> = [];
    for (const doc of docs) {
      for (const diag of diagnosticsFor(doc.id)) {
        const sev = diag.severity;
        if (severity === "error" && sev !== "error") continue;
        if (severity === "warning" && sev !== "warning") continue;
        if (sourceFilter && !(diag.source ?? "").toLowerCase().includes(sourceFilter.toLowerCase())) continue;
        out.push({ docId: doc.id, path: doc.path, diag });
      }
    }
    // severity 순 정렬 (error 우선)
    out.sort((a, b) => severityRank(a.diag) - severityRank(b.diag));
    return out;
  }, [docs, diagnosticsFor, severity, sourceFilter]);

  const counts = useMemo(() => {
    let errors = 0;
    let warnings = 0;
    for (const doc of docs) {
      for (const diag of diagnosticsFor(doc.id)) {
        if (diag.severity === "error") errors += 1;
        else if (diag.severity === "warning") warnings += 1;
      }
    }
    return { errors, warnings };
  }, [docs, diagnosticsFor]);

  return (
    <div className="problems-panel">
      <div className="problems-head">
        <span className="problems-title">문제</span>
        <span className="problems-counts">
          {counts.errors > 0 && <span className="problems-errors">{counts.errors}</span>}
          {counts.warnings > 0 && <span className="problems-warnings">{counts.warnings}</span>}
        </span>
        <div className="problems-filters">
          <select value={severity} onChange={(e) => setSeverity(e.currentTarget.value as SeverityFilter)}>
            <option value="all">전체</option>
            <option value="error">오류</option>
            <option value="warning">경고</option>
          </select>
          <input placeholder="source" value={sourceFilter} onChange={(e) => setSourceFilter(e.currentTarget.value)} />
        </div>
        <button className="mini" onClick={onClose}>✕</button>
      </div>

      <div className="problems-server">
        {serverStatus ? `LSP: ${serverStatus}` : "LSP 연결 없음"}
      </div>

      <div className="problems-list">
        {rows.map(({ docId, path, diag }, i) => (
          <div
            key={`${docId}-${i}`}
            className={`problem-row severity-${diag.severity}`}
            onClick={() => onNavigate(docId, diag.from)}
          >
            <span className="problem-severity">{diag.severity === "error" ? "●" : "▲"}</span>
            <span className="problem-msg">{diag.message}</span>
            <span className="problem-meta">{diag.source ?? ""}</span>
            <span className="problem-path">{path}</span>
          </div>
        ))}
        {rows.length === 0 && <div className="empty">진단 결과가 없습니다.</div>}
      </div>
    </div>
  );
}

function severityRank(diag: Diagnostic): number {
  return diag.severity === "error" ? 0 : diag.severity === "warning" ? 1 : 2;
}
