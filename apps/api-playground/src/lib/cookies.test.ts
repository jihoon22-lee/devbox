import { describe, expect, it } from "vitest";
import {
  addCookie,
  buildCookieHeader,
  cookieSecretReference,
  duplicateCookie,
  hasCookieSourceConflict,
  MAX_REQUEST_COOKIE_ROWS,
  normalizeCookies,
  removeCookie,
  updateCookie,
  validateCookies,
} from "./cookies";

describe("request cookie rows", () => {
  it("기존 enabled 누락은 활성으로 정규화하고 편집은 원본을 변경하지 않는다", () => {
    const source = [{ name: "session", value: "abc" }];
    expect(normalizeCookies(source)).toEqual([{ name: "session", value: "abc", enabled: true }]);
    expect(updateCookie(source, 0, { enabled: false })).toEqual([
      { name: "session", value: "abc", enabled: false },
    ]);
    expect(source).toEqual([{ name: "session", value: "abc" }]);
    expect(duplicateCookie(source, 0)).toHaveLength(2);
    expect(removeCookie(source, 0)).toEqual([]);
  });

  it("행 수를 100개로 제한한다", () => {
    const full = Array.from({ length: MAX_REQUEST_COOKIE_ROWS }, (_, index) => ({
      name: `c${index}`,
      value: String(index),
    }));
    expect(addCookie(full)).toHaveLength(MAX_REQUEST_COOKIE_ROWS);
    expect(normalizeCookies([...full, { name: "overflow", value: "x" }])).toHaveLength(
      MAX_REQUEST_COOKIE_ROWS,
    );
    expect(validateCookies([...full, { name: "overflow", value: "x" }])).toContainEqual({
      index: MAX_REQUEST_COOKIE_ROWS,
      message: "Cookie는 최대 100행까지 사용할 수 있습니다.",
    });
  });

  it("빈 행과 disabled 행은 허용하고 잘못된 이름·값 위치를 보고한다", () => {
    expect(validateCookies([
      { name: "", value: "" },
      { name: "bad name", value: "x" },
      { name: "ok", value: "has space" },
      { name: "ignored", value: "has;semicolon", enabled: false },
    ])).toEqual([
      { index: 1, message: "이름에 Cookie token으로 쓸 수 없는 문자가 있습니다." },
      { index: 2, message: "값에 공백, 세미콜론, 따옴표 또는 제어 문자를 사용할 수 없습니다." },
    ]);
  });

  it("활성 cookie를 순서대로 조립하고 직접 값만 마스킹한다", () => {
    const rows = [
      { name: "session", value: "plain" },
      { name: "token", value: "${COOKIE_TOKEN}" },
      { name: "mixed", value: "prefix-${COOKIE_TOKEN}" },
      { name: "empty", value: "" },
      { name: "skip", value: "hidden", enabled: false },
    ];
    expect(buildCookieHeader(rows)).toBe(
      "session=plain; token=${COOKIE_TOKEN}; mixed=prefix-${COOKIE_TOKEN}; empty=",
    );
    expect(buildCookieHeader(rows, true)).toBe(
      "session=[REDACTED]; token=${COOKIE_TOKEN}; mixed=[REDACTED]; empty=",
    );
  });

  it("구조화 cookie와 활성 raw Cookie header의 충돌만 감지한다", () => {
    const cookies = [{ name: "session", value: "x" }];
    expect(hasCookieSourceConflict(cookies, [{ key: "Cookie", value: "legacy=x" }])).toBe(true);
    expect(hasCookieSourceConflict(cookies, [
      { key: "cookie", value: "legacy=x", enabled: false },
    ])).toBe(false);
    expect(hasCookieSourceConflict([], [{ key: "Cookie", value: "legacy=x" }])).toBe(false);
  });

  it("유효한 secret 이름만 reference로 만든다", () => {
    expect(cookieSecretReference("SESSION_TOKEN")).toBe("${SESSION_TOKEN}");
    expect(cookieSecretReference("bad name")).toBe("");
  });
});
