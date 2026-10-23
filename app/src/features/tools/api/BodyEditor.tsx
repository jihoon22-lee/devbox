/** 본문 편집: 종류마다 초안을 따로 두고(AR-F06) 활성 종류만 보낸다. */
import { CodeView } from "../../../ui/code/CodeView";
import s from "./Api.module.css";
import { type BodyKind, type PartRow, type RequestDraft, rowId, setBodyKind, updateBody } from "./model/draft";
import { ParamTable } from "./ParamTable";

const KINDS: [BodyKind, string][] = [
  ["none", "없음"],
  ["json", "JSON"],
  ["raw", "텍스트"],
  ["form", "form"],
  ["multipart", "multipart"],
  ["binary", "파일"],
  ["graphql", "GraphQL"],
];

export function BodyEditor({ draft, onChange }: { draft: RequestDraft; onChange: (draft: RequestDraft) => void }) {
  const kind = draft.bodyKind;
  return (
    <div className={s.table}>
      <label className={s.bar}>
        본문 종류
        <select
          className={s.select}
          value={kind}
          onChange={(event) => onChange(setBodyKind(draft, event.target.value as BodyKind))}
        >
          {KINDS.map(([value, label]) => (
            <option key={value} value={value}>
              {label}
            </option>
          ))}
        </select>
      </label>
      {kind === "json" && (
        <CodeView
          ariaLabel="JSON 본문"
          language="json"
          value={draft.bodies.json?.text ?? ""}
          onChange={(text) => onChange(updateBody(draft, "json", { text }))}
        />
      )}
      {kind === "raw" && (
        <>
          <input
            className={s.input}
            aria-label="Content-Type"
            value={draft.bodies.raw?.contentType ?? ""}
            onChange={(event) => onChange(updateBody(draft, "raw", { contentType: event.target.value }))}
          />
          <CodeView
            ariaLabel="텍스트 본문"
            value={draft.bodies.raw?.text ?? ""}
            onChange={(text) => onChange(updateBody(draft, "raw", { text }))}
          />
        </>
      )}
      {kind === "form" && (
        <ParamTable
          label="form"
          rows={draft.bodies.form?.fields ?? []}
          onChange={(fields) => onChange(updateBody(draft, "form", { fields }))}
        />
      )}
      {kind === "multipart" && (
        <Parts
          parts={draft.bodies.multipart?.parts ?? []}
          onChange={(parts) => onChange(updateBody(draft, "multipart", { parts }))}
        />
      )}
      {kind === "binary" && (
        <input
          className={`${s.input} ${s.mono}`}
          aria-label="보낼 파일 경로(WSL)"
          placeholder="/home/…"
          value={draft.bodies.binary?.path ?? ""}
          onChange={(event) => onChange(updateBody(draft, "binary", { path: event.target.value }))}
        />
      )}
      {kind === "graphql" && <Graphql draft={draft} onChange={onChange} />}
    </div>
  );
}

function Graphql({ draft, onChange }: { draft: RequestDraft; onChange: (draft: RequestDraft) => void }) {
  const body = draft.bodies.graphql;
  return (
    <>
      <CodeView
        ariaLabel="GraphQL 쿼리"
        value={body?.query ?? ""}
        onChange={(query) => onChange(updateBody(draft, "graphql", { query }))}
      />
      <CodeView
        ariaLabel="GraphQL 변수"
        language="json"
        value={body?.variables ?? ""}
        onChange={(variables) => onChange(updateBody(draft, "graphql", { variables }))}
      />
      <div className={s.bar}>
        <input
          className={s.input}
          aria-label="operationName"
          value={body?.operationName ?? ""}
          onChange={(event) => onChange(updateBody(draft, "graphql", { operationName: event.target.value }))}
        />
        <select
          className={s.select}
          aria-label="GraphQL 보내는 방식"
          value={body?.method ?? "post"}
          onChange={(event) => onChange(updateBody(draft, "graphql", { method: event.target.value as "post" | "get" }))}
        >
          <option value="post">POST</option>
          <option value="get">GET</option>
        </select>
      </div>
    </>
  );
}

function Parts({ parts, onChange }: { parts: PartRow[]; onChange: (parts: PartRow[]) => void }) {
  const patch = (index: number, value: Partial<PartRow>) =>
    onChange(parts.map((part, i) => (i === index ? { ...part, ...value } : part)));
  return (
    <div className={s.table}>
      {parts.map((part, index) => (
        <div key={part.id} role="group" aria-label={`파트 ${index + 1}`} className={s.bar}>
          <input
            type="checkbox"
            aria-label={`파트 ${index + 1} 사용`}
            checked={part.enabled}
            onChange={(event) => patch(index, { enabled: event.target.checked })}
          />
          <select
            className={s.select}
            aria-label={`파트 ${index + 1} 종류`}
            value={part.kind}
            onChange={(event) => patch(index, { kind: event.target.value as PartRow["kind"] })}
          >
            <option value="text">텍스트</option>
            <option value="file">파일</option>
          </select>
          <input
            className={s.input}
            aria-label={`파트 ${index + 1} 이름`}
            value={part.name}
            onChange={(event) => patch(index, { name: event.target.value })}
          />
          <input
            className={`${s.input} ${s.mono} ${s.grow}`}
            aria-label={part.kind === "file" ? `파트 ${index + 1} 파일 경로` : `파트 ${index + 1} 값`}
            value={part.kind === "file" ? part.path : part.value}
            onChange={(event) =>
              patch(index, part.kind === "file" ? { path: event.target.value } : { value: event.target.value })
            }
          />
          <button
            type="button"
            aria-label={`파트 ${index + 1} 삭제`}
            onClick={() => onChange(parts.filter((_, i) => i !== index))}
          >
            삭제
          </button>
        </div>
      ))}
      <div>
        <button
          type="button"
          onClick={() =>
            onChange([
              ...parts,
              {
                id: rowId(),
                enabled: true,
                kind: "text",
                name: "",
                value: "",
                path: "",
                fileName: "",
                contentType: "",
              },
            ])
          }
        >
          파트 추가
        </button>
      </div>
    </div>
  );
}
