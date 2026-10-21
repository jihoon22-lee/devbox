import type { EnvVariable, RequestTemplate } from "../template";
import { go } from "./go";
import { axios, fetchCode } from "./javascript";
import { prepare, singleLine } from "./plan";
import { python } from "./python";
import { rust } from "./rust";
import { curl } from "./shell";

/** v0.9.0의 C#은 07 대상 언어가 아니라 빼고 axios·Rust reqwest를 더했다. */
export type CodeTarget = "curl" | "fetch" | "axios" | "python" | "go" | "rust";
export const CODE_TARGETS: readonly CodeTarget[] = ["curl", "fetch", "axios", "python", "go", "rust"];

export function generateCode(
  target: CodeTarget,
  request: RequestTemplate,
  environment: EnvVariable[],
): { code: string; placeholders: string[] } {
  const placeholders = new Set<string>();
  const plan = prepare(request, environment, placeholders);
  const comments: string[] = [];
  if (plan.credential && target !== "fetch") {
    if (plan.verify) placeholders.add("ca_pem");
    placeholders.add("client_cert_pem");
    placeholders.add("client_key_pem");
    comments.push(
      "TLS 자격 증명은 내보내지 않습니다. PEM 파일 경로를 채우고 사용하지 않는 CA 또는 클라이언트 인증서 설정은 제거하세요.",
    );
  }
  if (!plan.verify) comments.push("인증서 검증 꺼짐");
  if (target === "fetch") comments.push("브라우저의 CORS·금지 헤더·쿠키 정책이 적용됩니다.");
  if (plan.headers.some(([key]) => key.toLowerCase() === "authorization") && request.auth?.kind === "basic")
    comments.push("basic_credentials_base64에는 사용자 이름:비밀번호의 UTF-8 base64를 넣으세요.");
  const generator = { curl, fetch: fetchCode, axios, python, go, rust }[target];
  const code = generator(plan);
  const names = [...placeholders].sort();
  if (names.length) comments.unshift(`채워 넣을 값: ${names.map((name) => `{{${name}}}`).join(", ")}`);
  const prefix = target === "curl" || target === "python" ? "#" : "//";
  return { code: [...comments.map((line) => `${prefix} ${singleLine(line)}`), code].join("\n"), placeholders: names };
}
