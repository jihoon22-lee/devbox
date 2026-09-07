import type { RequestHeader } from "./types";
import {
  addHeader,
  availableSecretNames,
  duplicateHeader,
  duplicateHeaderNameCount,
  isHeaderEnabled,
  MAX_REQUEST_HEADER_ROWS,
  removeHeader,
  secretReference,
  updateHeader,
} from "./lib/headers";

export function HeaderTable({
  rows,
  secretNames,
  onChange,
}: {
  rows: RequestHeader[];
  secretNames: string[];
  onChange: (rows: RequestHeader[]) => void;
}) {
  const availableSecrets = availableSecretNames(secretNames);
  const enabledCount = rows.filter(isHeaderEnabled).length;
  const duplicateCount = duplicateHeaderNameCount(rows);

  return (
    <div className="header-editor">
      <div className="header-summary" aria-live="polite">
        활성 {enabledCount} / 전체 {rows.length}
        {duplicateCount > 0 ? ` · 중복 이름 ${duplicateCount}개` : ""}
      </div>

      <div className="header-table" role="table" aria-label="요청 Header 편집">
        {rows.map((row, index) => (
          <div
            className={`header-row ${isHeaderEnabled(row) ? "" : "disabled"}`}
            role="row"
            key={index}
          >
            <label className="header-enabled">
              <input
                aria-label={`${index + 1}번 header 활성화`}
                type="checkbox"
                checked={isHeaderEnabled(row)}
                onChange={(event) => onChange(updateHeader(rows, index, { enabled: event.currentTarget.checked }))}
              />
              사용
            </label>
            <input
              aria-label={`${index + 1}번 header 이름`}
              placeholder="헤더 이름"
              value={row.key}
              onChange={(event) => onChange(updateHeader(rows, index, { key: event.currentTarget.value }))}
              spellCheck={false}
            />
            <input
              aria-label={`${index + 1}번 header 값`}
              placeholder="Value 또는 ${SECRET_NAME}"
              value={row.value}
              onChange={(event) => onChange(updateHeader(rows, index, { value: event.currentTarget.value }))}
              spellCheck={false}
            />
            <select
              aria-label={`${index + 1}번 header secret 참조`}
              title="현재 환경의 봉인된 secret 이름으로 행 값 전체를 교체"
              value=""
              disabled={availableSecrets.length === 0}
              onChange={(event) => {
                const reference = secretReference(event.currentTarget.value);
                if (reference) onChange(updateHeader(rows, index, { value: reference }));
              }}
            >
              <option value="">
                {availableSecrets.length === 0 ? "Secret 없음" : "Secret 참조"}
              </option>
              {availableSecrets.map((name) => <option key={name} value={name}>{name}</option>)}
            </select>
            <button
              type="button"
              className="btn header-row-action"
              aria-label={`${index + 1}번 header 복제`}
              disabled={rows.length >= MAX_REQUEST_HEADER_ROWS}
              onClick={() => onChange(duplicateHeader(rows, index))}
            >
              복제
            </button>
            <button
              type="button"
              className="kv-del"
              aria-label={`${index + 1}번 header 삭제`}
              onClick={() => onChange(removeHeader(rows, index))}
            >
              ✕
            </button>
          </div>
        ))}
      </div>

      <button
        type="button"
        className="btn kv-add"
        disabled={rows.length >= MAX_REQUEST_HEADER_ROWS}
        onClick={() => onChange(addHeader(rows))}
      >
        + 헤더 추가
      </button>

      <div className="header-notice" role="note">
        같은 이름의 header를 여러 행으로 유지하며 순서대로 전송합니다. 사용을 끈 행은 History와
        Collection에는 남지만 요청과 cURL에서는 제외됩니다. Secret 참조는 현재 환경의 봉인된
        이름으로 행 값 전체를 바꾸며, 봉인된 원문을 읽거나 표시하지 않습니다. 필요한 접두사는
        값 입력에서 추가할 수 있습니다. 요청 header는 최대 100행입니다.
      </div>
    </div>
  );
}
