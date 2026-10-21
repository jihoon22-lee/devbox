// v0.9.0 Devbox 내보내기 문서는 가져오지 않는다(CONVENTIONS §9). 01 §2.4의 형식만 감지한다.

import { parseBruno } from "./bruno";
import { parseCurl } from "./curl";
import { parseHar } from "./har";
import { parseInsomnia } from "./insomnia";
import { type ImportBundle, ImportError, type ImportFormat } from "./model";
import { parsePostman } from "./postman";

export * from "./model";
export { type ImportPreview, type PreviewEnvironment, type PreviewRequest, toImportPreview } from "./preview";
export function detectFormat(fileName: string, text: string): ImportFormat | null {
  if (/\.bru$/i.test(fileName)) return "bruno";
  if (/\.har$/i.test(fileName)) return "har";
  try {
    if (new TextEncoder().encode(text).length > 16 * 1024 * 1024) return null;
    const value = JSON.parse(text.replace(/^\uFEFF/, ""));
    if (!value || typeof value !== "object" || Array.isArray(value)) return null;
    if (Array.isArray(value.log?.entries)) return "har";
    if (typeof value.info?.schema === "string" && value.info.schema.includes("getpostman")) return "postman";
    if (value._type === "export") return "insomnia";
  } catch {}
  return null;
}
export function parseImport(format: ImportFormat, files: { relativePath: string; text: string }[]): ImportBundle {
  try {
    if (!files.length || files.length > 500) throw new ImportError("가져올 수 없는 형식입니다.");
    let total = 0;
    for (const file of files) {
      const bytes = new TextEncoder().encode(file.text).length;
      if (bytes > 16 * 1024 * 1024) throw new ImportError("가져올 수 없는 형식입니다.");
      total += bytes;
    }
    if (total > 32 * 1024 * 1024) throw new ImportError("가져올 수 없는 형식입니다.");
    if (format === "bruno") return parseBruno(files);
    const combined: ImportBundle = { requests: [], environments: [], warnings: [] };
    for (const file of files) {
      const source = file.text.replace(/^\uFEFF/, "");
      let bundle: ImportBundle;
      switch (format) {
        case "curl":
          bundle = parseCurl(source);
          break;
        case "postman":
          bundle = parsePostman(source);
          break;
        case "insomnia":
          bundle = parseInsomnia(source);
          break;
        case "har":
          bundle = parseHar(source);
          break;
        default:
          throw new ImportError("가져올 수 없는 형식입니다.");
      }
      combined.requests.push(...bundle.requests);
      combined.environments.push(...bundle.environments);
      combined.warnings.push(...bundle.warnings);
    }
    return combined;
  } catch {
    throw new ImportError("가져올 수 없는 형식입니다.");
  }
}
