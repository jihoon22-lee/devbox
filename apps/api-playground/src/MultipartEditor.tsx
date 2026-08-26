import { useRef, useState } from "react";
import { availableSecretNames, secretReference } from "./lib/headers";
import {
  addMultipartPart,
  duplicateMultipartPart,
  isMultipartPartEnabled,
  MAX_MULTIPART_PARTS,
  removeMultipartPart,
  setMultipartFile,
  updateMultipartPart,
  validateMultipartParts,
  type PickedMultipartFile,
} from "./lib/multipart";
import type { MultipartPart } from "./types";

export function MultipartEditor({
  rows,
  secretNames,
  onChange,
  onPickFile,
}: {
  rows: MultipartPart[];
  secretNames: string[];
  onChange: (rows: MultipartPart[]) => void;
  onPickFile: () => Promise<PickedMultipartFile | null>;
}) {
  const [picking, setPicking] = useState<number | null>(null);
  const [pickError, setPickError] = useState<string | null>(null);
  const latestRows = useRef(rows);
  latestRows.current = rows;
  const availableSecrets = availableSecretNames(secretNames);
  const issues = validateMultipartParts(rows);
  const issuesByRow = new Map<number, string[]>();
  for (const issue of issues) {
    issuesByRow.set(issue.index, [...(issuesByRow.get(issue.index) ?? []), issue.message]);
  }

  const pick = async (index: number) => {
    setPicking(index);
    setPickError(null);
    try {
      const file = await onPickFile();
      if (file) onChange(setMultipartFile(latestRows.current, index, file));
    } catch {
      setPickError("파일을 선택하지 못했습니다. 데스크톱 앱 권한을 확인하세요.");
    } finally {
      setPicking(null);
    }
  };

  return (
    <div className="multipart-editor">
      <div className="header-summary" aria-live="polite">
        전송 {rows.filter((row) => isMultipartPartEnabled(row) && Boolean(row.name)).length} / 전체 {rows.length}
      </div>
      <div className="multipart-table" role="table" aria-label="multipart part 편집">
        {rows.map((row, index) => (
          <div
            className={`multipart-row ${isMultipartPartEnabled(row) ? "" : "disabled"} ${issuesByRow.has(index) ? "invalid" : ""}`}
            role="row"
            key={index}
          >
            <label className="header-enabled">
              <input
                aria-label={`${index + 1}번 part 활성화`}
                type="checkbox"
                checked={isMultipartPartEnabled(row)}
                onChange={(event) => onChange(updateMultipartPart(rows, index, { enabled: event.currentTarget.checked }))}
              />
              사용
            </label>
            <select
              aria-label={`${index + 1}번 part 종류`}
              value={row.kind}
              onChange={(event) => onChange(updateMultipartPart(rows, index, { kind: event.currentTarget.value as MultipartPart["kind"] }))}
            >
              <option value="text">Text</option>
              <option value="file">File</option>
            </select>
            <input
              aria-label={`${index + 1}번 part 이름`}
              placeholder="Part name"
              value={row.name}
              onChange={(event) => onChange(updateMultipartPart(rows, index, { name: event.currentTarget.value }))}
              spellCheck={false}
            />
            {row.kind === "text" ? (
              <input
                aria-label={`${index + 1}번 text 값`}
                placeholder="Value 또는 ${SECRET_NAME}"
                value={row.value}
                onChange={(event) => onChange(updateMultipartPart(rows, index, { value: event.currentTarget.value }))}
                spellCheck={false}
              />
            ) : (
              <button
                type="button"
                className="btn multipart-file"
                aria-label={`${index + 1}번 파일 선택`}
                title={row.file_name || "파일을 선택하세요"}
                disabled={picking !== null}
                onClick={() => void pick(index)}
              >
                {picking === index ? "선택 중..." : row.file_name || "파일 선택"}
              </button>
            )}
            <input
              aria-label={`${index + 1}번 part Content-Type`}
              placeholder="Content-Type (선택)"
              value={row.content_type}
              onChange={(event) => onChange(updateMultipartPart(rows, index, { content_type: event.currentTarget.value }))}
              spellCheck={false}
            />
            {row.kind === "text" ? (
              <select
                aria-label={`${index + 1}번 part secret 참조`}
                value=""
                disabled={availableSecrets.length === 0}
                onChange={(event) => {
                  const reference = secretReference(event.currentTarget.value);
                  if (reference) onChange(updateMultipartPart(rows, index, { value: reference }));
                }}
              >
                <option value="">{availableSecrets.length === 0 ? "Secret 없음" : "Secret 참조"}</option>
                {availableSecrets.map((name) => <option key={name} value={name}>{name}</option>)}
              </select>
            ) : <span />}
            <button
              type="button"
              className="btn header-row-action"
              aria-label={`${index + 1}번 part 복제`}
              disabled={rows.length >= MAX_MULTIPART_PARTS}
              onClick={() => onChange(duplicateMultipartPart(rows, index))}
            >
              복제
            </button>
            <button
              type="button"
              className="kv-del"
              aria-label={`${index + 1}번 part 삭제`}
              onClick={() => onChange(removeMultipartPart(rows, index))}
            >
              ✕
            </button>
            {issuesByRow.has(index) && (
              <div className="multipart-row-error" role="alert">
                {issuesByRow.get(index)?.join(" ")}
              </div>
            )}
          </div>
        ))}
      </div>
      {pickError && <div className="multipart-row-error" role="alert">{pickError}</div>}
      <div className="multipart-add-actions">
        <button type="button" className="btn" disabled={rows.length >= MAX_MULTIPART_PARTS} onClick={() => onChange(addMultipartPart(rows, "text"))}>+ Text</button>
        <button type="button" className="btn" disabled={rows.length >= MAX_MULTIPART_PARTS} onClick={() => onChange(addMultipartPart(rows, "file"))}>+ File</button>
      </div>
      <div className="header-notice" role="note">
        선택한 파일은 앱에 복사하거나 History·Collection에 저장하지 않고 전송할 때만 읽습니다.
        저장된 요청의 파일은 다시 선택해야 하며 파일당 25 MiB, 전체 50 MiB, 최대 50개 part까지 전송합니다.
      </div>
    </div>
  );
}
