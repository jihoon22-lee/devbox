import { useState } from "react";
import {
  addCookie,
  cookieSecretReference,
  duplicateCookie,
  isCookieEnabled,
  MAX_REQUEST_COOKIE_ROWS,
  removeCookie,
  updateCookie,
  validateCookies,
} from "./lib/cookies";
import { availableSecretNames } from "./lib/headers";
import type { RequestCookie } from "./types";

export function CookieEditor({
  rows,
  secretNames,
  hasRawCookieHeader,
  onChange,
}: {
  rows: RequestCookie[];
  secretNames: string[];
  hasRawCookieHeader: boolean;
  onChange: (rows: RequestCookie[]) => void;
}) {
  const [revealedRows, setRevealedRows] = useState<Set<number>>(() => new Set());
  const availableSecrets = availableSecretNames(secretNames);
  const issues = validateCookies(rows);
  const issueByRow = new Map(issues.map((issue) => [issue.index, issue.message]));
  const activeCount = rows.filter(
    (row) => isCookieEnabled(row) && Boolean(row.name || row.value),
  ).length;

  const toggleReveal = (index: number) => {
    setRevealedRows((current) => {
      const next = new Set(current);
      if (next.has(index)) next.delete(index);
      else next.add(index);
      return next;
    });
  };

  const hideRow = (index: number) => {
    setRevealedRows((current) => {
      if (!current.has(index)) return current;
      const next = new Set(current);
      next.delete(index);
      return next;
    });
  };

  const resetReveal = () => setRevealedRows(new Set());

  return (
    <div className="cookie-editor">
      <div className="header-summary" aria-live="polite">
        전송 {activeCount} / 전체 {rows.length}
      </div>

      {hasRawCookieHeader && activeCount > 0 && (
        <div className="cookie-conflict" role="alert">
          활성 Cookie header와 구조화 Cookie를 동시에 전송할 수 없습니다. Headers 탭의 Cookie
          행을 끄거나 삭제하세요.
        </div>
      )}

      <div className="cookie-table" role="table" aria-label="요청 Cookie 편집">
        {rows.map((row, index) => {
          const issue = issueByRow.get(index);
          const revealed = revealedRows.has(index);
          return (
            <div
              className={`cookie-row ${isCookieEnabled(row) ? "" : "disabled"} ${issue ? "invalid" : ""}`}
              role="row"
              key={index}
            >
              <label className="header-enabled">
                <input
                  aria-label={`${index + 1}번 cookie 활성화`}
                  type="checkbox"
                  checked={isCookieEnabled(row)}
                  onChange={(event) => {
                    if (!event.currentTarget.checked) hideRow(index);
                    onChange(updateCookie(rows, index, { enabled: event.currentTarget.checked }));
                  }}
                />
                사용
              </label>
              <input
                aria-label={`${index + 1}번 cookie 이름`}
                placeholder="Cookie name"
                value={row.name}
                onChange={(event) =>
                  onChange(updateCookie(rows, index, { name: event.currentTarget.value }))
                }
                spellCheck={false}
              />
              <input
                aria-label={`${index + 1}번 cookie 값`}
                placeholder="Value 또는 ${SECRET_NAME}"
                type={revealed ? "text" : "password"}
                autoComplete="off"
                value={row.value}
                onChange={(event) =>
                  onChange(updateCookie(rows, index, { value: event.currentTarget.value }))
                }
                spellCheck={false}
              />
              <button
                type="button"
                className="btn header-row-action"
                aria-label={`${index + 1}번 cookie 값 ${revealed ? "숨김" : "보기"}`}
                onClick={() => toggleReveal(index)}
              >
                {revealed ? "숨김" : "보기"}
              </button>
              <select
                aria-label={`${index + 1}번 cookie secret 참조`}
                title="현재 환경의 봉인된 secret 이름으로 cookie 값 전체를 교체"
                value=""
                disabled={availableSecrets.length === 0}
                onChange={(event) => {
                  const reference = cookieSecretReference(event.currentTarget.value);
                  if (reference) {
                    hideRow(index);
                    onChange(updateCookie(rows, index, { value: reference }));
                  }
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
                aria-label={`${index + 1}번 cookie 복제`}
                disabled={rows.length >= MAX_REQUEST_COOKIE_ROWS}
                onClick={() => {
                  resetReveal();
                  onChange(duplicateCookie(rows, index));
                }}
              >
                복제
              </button>
              <button
                type="button"
                className="kv-del"
                aria-label={`${index + 1}번 cookie 삭제`}
                onClick={() => {
                  resetReveal();
                  onChange(removeCookie(rows, index));
                }}
              >
                ✕
              </button>
              {issue && <div className="cookie-row-error">{issue}</div>}
            </div>
          );
        })}
      </div>

      <button
        type="button"
        className="btn kv-add"
        disabled={rows.length >= MAX_REQUEST_COOKIE_ROWS}
        onClick={() => {
          resetReveal();
          onChange(addCookie(rows));
        }}
      >
        + Cookie 추가
      </button>

      <div className="header-notice" role="note">
        이 편집기는 domain/path/만료일을 관리하는 브라우저 cookie jar가 아니라 현재 요청의 Cookie
        header만 만듭니다. 값은 기본적으로 숨기며 직접 입력한 값은 History·Collection·기본 cURL에
        평문 저장하지 않습니다. 봉인된 secret은 이름 참조만 삽입합니다. Cookie는 최대 100행이며
        세미콜론으로 구분되는 하나의 요청 header로 순서대로 전송합니다.
      </div>
    </div>
  );
}
