import { TransformerTool } from "./common";

function ok(output: string) {
  return Promise.resolve({ output });
}
function fail(e: unknown) {
  return Promise.resolve({ output: "", error: e instanceof Error ? e.message : String(e) });
}

function formatJson(input: string, mode: "format" | "minify") {
  try {
    const parsed = JSON.parse(input);
    const json = JSON.stringify(parsed, null, mode === "format" ? 2 : 0);
    return ok(json);
  } catch (e) {
    return fail(e);
  }
}

function toBase64(input: string) {
  try {
    return ok(btoa(unescape(encodeURIComponent(input))));
  } catch (e) {
    return fail(e);
  }
}

function fromBase64(input: string) {
  try {
    return ok(decodeURIComponent(escape(atob(input.trim()))));
  } catch (e) {
    return fail(e);
  }
}

function decodeJwt(input: string) {
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

export function UrlEncoder() {
  return <TransformerTool placeholder="Text to URL-encode..." run={(i) => ok(encodeURIComponent(i))} />;
}
export function UrlDecoder() {
  return <TransformerTool placeholder="URL-encoded text..." run={(i) => ok(decodeURIComponent(i))} />;
}

export function TimestampConverter() {
  return (
    <TransformerTool
      placeholder="Unix timestamp (seconds) or ISO date..."
      run={(i) => {
        const t = i.trim();
        if (!t) return ok("");
        const num = Number(t);
        const date = Number.isFinite(num)
          ? new Date(num < 1e12 ? num * 1000 : num)
          : new Date(t);
        if (Number.isNaN(date.getTime())) return fail(new Error("날짜로 해석할 수 없습니다"));
        return ok(date.toLocaleString());
      }}
    />
  );
}

export function CaseConverter() {
  return (
    <TransformerTool
      placeholder="Text to convert..."
      run={(i) => {
        const camel = i.replace(/(?:^\w|[A-Z]|\b\w)/g, (w, idx) =>
          idx === 0 ? w.toLowerCase() : w.toUpperCase(),
        ).replace(/\s+/g, "");
        const kebab = i.toLowerCase().replace(/[\s_]+/g, "-");
        const snake = i.toLowerCase().replace(/[\s-]+/g, "_");
        const pascal = camel.charAt(0).toUpperCase() + camel.slice(1);
        const upper = i.toUpperCase();
        const lower = i.toLowerCase();
        return ok(
          [
            `UPPER: ${upper}`,
            `lower: ${lower}`,
            `Pascal: ${pascal}`,
            `camel: ${camel}`,
            `kebab: ${kebab}`,
            `snake: ${snake}`,
          ].join("\n"),
        );
      }}
    />
  );
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
