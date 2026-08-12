import { TransformerTool } from "./common";

// 반환 타입을 명시해 ok/fail 두 분기가 항상 같은 모양({ output, error? })을 갖게 한다.
// (동작 변화 없음 — ok()가 error를 안 채우는 건 원래와 동일하고, 타입에서만 optional로 통일한다.)
type TransformResult = { output: string; error?: string };

function ok(output: string): Promise<TransformResult> {
  return Promise.resolve({ output });
}
function fail(e: unknown): Promise<TransformResult> {
  return Promise.resolve({ output: "", error: e instanceof Error ? e.message : String(e) });
}

// 아래 순수 함수들은 테스트를 위해 export한다 (동작은 바꾸지 않음).

export function formatJson(input: string, mode: "format" | "minify") {
  try {
    const parsed = JSON.parse(input);
    const json = JSON.stringify(parsed, null, mode === "format" ? 2 : 0);
    return ok(json);
  } catch (e) {
    return fail(e);
  }
}

export function toBase64(input: string) {
  try {
    return ok(btoa(unescape(encodeURIComponent(input))));
  } catch (e) {
    return fail(e);
  }
}

export function fromBase64(input: string) {
  try {
    return ok(decodeURIComponent(escape(atob(input.trim()))));
  } catch (e) {
    return fail(e);
  }
}

export function decodeJwt(input: string) {
  try {
    const [, payloadB64] = input.trim().split(".");
    if (!payloadB64) throw new Error("JWT 페이로드가 없습니다 (header.payload.signature 형식)");
    const json = decodeURIComponent(escape(atob(payloadB64.replace(/-/g, "+").replace(/_/g, "/"))));
    return ok(JSON.stringify(JSON.parse(json), null, 2));
  } catch (e) {
    return fail(e);
  }
}

export const jsonFormatter = () => (input: string) => formatJson(input, "format");
export const jsonMinifier = () => (input: string) => formatJson(input, "minify");
export const base64Encode = () => toBase64;
export const base64Decode = () => fromBase64;

/** URL 인코딩/디코딩: 컴포넌트에 인라인돼 있던 로직을 순수 함수로 추출했다.
 *  원래 코드처럼 try/catch가 없다 — decodeURIComponent가 잘못된 % 시퀀스에서
 *  던지는 예외는 그대로 전파된다(동작 불변). */
export function urlEncode(input: string): string {
  return encodeURIComponent(input);
}
export function urlDecode(input: string): string {
  return decodeURIComponent(input);
}

export function UrlEncoder() {
  return <TransformerTool placeholder="Text to URL-encode..." run={(i) => ok(urlEncode(i))} />;
}
export function UrlDecoder() {
  return <TransformerTool placeholder="URL-encoded text..." run={(i) => ok(urlDecode(i))} />;
}

/** 타임스탬프 문자열을 Date로 해석한다. 1e12 미만이면 초 단위로 보고 ms로 환산하고,
 *  숫자로 못 읽으면 날짜 문자열로 파싱을 시도한다. 해석 불가하면 null. */
export function parseTimestampInput(input: string): Date | null {
  const num = Number(input);
  const date = Number.isFinite(num) ? new Date(num < 1e12 ? num * 1000 : num) : new Date(input);
  return Number.isNaN(date.getTime()) ? null : date;
}

export function convertTimestamp(input: string) {
  const t = input.trim();
  if (!t) return ok("");
  const date = parseTimestampInput(t);
  if (!date) return fail(new Error("날짜로 해석할 수 없습니다"));
  return ok(date.toLocaleString());
}

export function TimestampConverter() {
  return <TransformerTool placeholder="Unix timestamp (seconds) or ISO date..." run={convertTimestamp} />;
}

/** UPPER/lower/Pascal/camel/kebab/snake 6종 표기를 한 번에 계산해 여러 줄 문자열로 합친다. */
export function convertCase(input: string): string {
  const camel = input
    .replace(/(?:^\w|[A-Z]|\b\w)/g, (w, idx) => (idx === 0 ? w.toLowerCase() : w.toUpperCase()))
    .replace(/\s+/g, "");
  const kebab = input.toLowerCase().replace(/[\s_]+/g, "-");
  const snake = input.toLowerCase().replace(/[\s-]+/g, "_");
  const pascal = camel.charAt(0).toUpperCase() + camel.slice(1);
  const upper = input.toUpperCase();
  const lower = input.toLowerCase();
  return [
    `UPPER: ${upper}`,
    `lower: ${lower}`,
    `Pascal: ${pascal}`,
    `camel: ${camel}`,
    `kebab: ${kebab}`,
    `snake: ${snake}`,
  ].join("\n");
}

export function CaseConverter() {
  return <TransformerTool placeholder="Text to convert..." run={(i) => ok(convertCase(i))} />;
}

export function JwtDecoder() {
  return (
    <TransformerTool
      placeholder="Paste JWT token (header.payload.signature)..."
      run={decodeJwt}
      rows={4}
    />
  );
}
