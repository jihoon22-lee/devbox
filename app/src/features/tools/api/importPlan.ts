/**
 * 가져오기 계획: 형식 감지(`importers/index.ts`, cURL·OpenAPI 포함) → 미리보기(가림·재검토 표시) → 만들 파일 경로.
 * 파일 텍스트는 고른 루트로 저장 정화를 다시 거친다(`serializeRequestFile`). OpenAPI URL은 `api.send` GET으로 받는다.
 */
import type { CollectionRoot } from "../../../rpc/gen/rpc";
import type { ApiBackend } from "./backend";
import {
  detectFormat,
  type ImportBundle,
  ImportError,
  type ImportFormat,
  parseImport,
  toImportPreview,
} from "./lib/importers";
import { parseOpenApiSource } from "./lib/openapi/preview";
import { detectOpenApiFormat, type OpenApiFormat } from "./lib/openapi/shared";
import { emptyDraft, toSnapshot } from "./model/draft";
import { envFilePath, environmentFromPreview, serializeEnvFile } from "./model/envFile";
import { importedFile } from "./model/fromTemplate";
import { requestFilePath, serializeRequestFile } from "./model/requestFile";

export interface PlannedFile {
  path: string;
  kind: "request" | "environment";
  review: boolean;
  text(root: CollectionRoot): string;
}
export interface ImportPlan {
  files: PlannedFile[];
  warnings: string[];
}

const FAILED = "가져올 수 없는 형식입니다.";

function openApiBundle(text: string, fileName: string, format?: OpenApiFormat): ImportBundle {
  const result = parseOpenApiSource(
    format ? { kind: "url", format, text } : { kind: "file", name: fileName || "openapi.yaml", text },
  );
  if (!result.ok) throw new ImportError(result.error.message);
  const operations = result.preview.operations.filter((op) => op.applyable);
  const warnings = result.preview.operations
    .filter((op) => !op.applyable)
    .map((op) => `${op.label}: ${op.errors[0]?.message ?? "가져올 수 없습니다"}`);
  return {
    requests: operations.map((op) => ({ name: op.label, folder: "", request: op.request })),
    environments: [],
    warnings,
  };
}

function bundleFor(text: string, fileName: string, openApiFormat?: OpenApiFormat): ImportBundle {
  if (openApiFormat) return openApiBundle(text, fileName, openApiFormat);
  let format: ImportFormat | null = detectFormat(fileName, text);
  if (!format && /^\s*curl\s/i.test(text)) format = "curl";
  if (format) return parseImport(format, [{ relativePath: fileName || "import.txt", text }]);
  if (/^\s*(?:["']?openapi["']?\s*:|\{[\s\S]*"openapi"\s*:)/m.test(text)) return openApiBundle(text, fileName);
  throw new ImportError(FAILED);
}

export function planImport(text: string, fileName = "", openApiFormat?: OpenApiFormat): ImportPlan {
  const preview = toImportPreview(bundleFor(text, fileName, openApiFormat));
  const used = new Set<string>();
  const requests = preview.requests.map((request): PlannedFile => {
    const file = importedFile(request);
    return {
      path: requestFilePath(request.name, request.folder, used),
      kind: "request",
      review: request.requiresSecretReview,
      text: (root) => serializeRequestFile(file, root).text,
    };
  });
  const environments = preview.environments.map((environment): PlannedFile => {
    const text = serializeEnvFile(environmentFromPreview(environment)).text;
    return { path: envFilePath(environment.name, used), kind: "environment", review: false, text: () => text };
  });
  if (!requests.length && !environments.length) throw new ImportError("가져올 요청이 없습니다.");
  return { files: [...requests, ...environments], warnings: preview.warnings };
}

export const importErrorText = (error: unknown) => (error instanceof ImportError ? error.message : FAILED);

/** OpenAPI 문서를 URL에서 받는다(데몬 송신 경로, 변수·비밀 없음). */
export async function fetchOpenApi(
  backend: Pick<ApiBackend, "send">,
  url: string,
  signal: AbortSignal,
): Promise<{ text: string; format: OpenApiFormat }> {
  const response = await backend.send(
    {
      snapshot: toSnapshot({ ...emptyDraft(), url }),
      env: { variables: [] },
      runId: null,
      origin: null,
      cookieJar: null,
    },
    signal,
  );
  if (response.status < 200 || response.status >= 300 || response.binary || response.truncated)
    throw new ImportError(`문서를 받지 못했습니다(HTTP ${response.status}).`);
  const path = (() => {
    try {
      return new URL(url).pathname;
    } catch {
      return url;
    }
  })();
  const text = new TextDecoder().decode(response.body);
  return { text, format: /\.json$/i.test(path) || /^\s*\{/.test(text) ? "json" : detectOpenApiFormat(path) };
}
