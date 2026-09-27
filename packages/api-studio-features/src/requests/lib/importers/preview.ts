import { COLLECTION_VERSION, type CollectionStore } from "../collections";
import { sanitizeRequestForPersistence } from "../persistence";
import {
  isSensitiveName,
  looksLikeSecret,
  MAX_EXPORTED_COLLECTIONS,
  MAX_EXPORTED_ENVIRONMENTS,
  MAX_EXPORTED_VARIABLES,
  MAX_TRANSFER_NAME_CHARS,
  parseEnvironmentExport,
  serializeEnvironmentExport,
  type EnvironmentExportDocument,
} from "../transfer";
import { ImportError, type ImportBundle } from "./model";
const name = (text: string, fallback: string) =>
  looksLikeSecret(text)
    ? fallback
    : text
        .replace(/[\u0000-\u001f\u007f]/g, " ")
        .trim()
        .slice(0, MAX_TRANSFER_NAME_CHARS) || fallback;
export function toImportPreview(
  bundle: ImportBundle,
  makeId: () => string,
): { collections: CollectionStore; environments: EnvironmentExportDocument; warnings: string[] } {
  const warnings = [...bundle.warnings];
  if (bundle.requests.length > MAX_EXPORTED_COLLECTIONS)
    warnings.push(`요청이 많아 처음 ${MAX_EXPORTED_COLLECTIONS}개만 가져왔습니다.`);
  if (bundle.environments.length > MAX_EXPORTED_ENVIRONMENTS)
    warnings.push(`환경이 많아 처음 ${MAX_EXPORTED_ENVIRONMENTS}개만 가져왔습니다.`);
  const collections: CollectionStore = {
    version: COLLECTION_VERSION,
    collections: bundle.requests.slice(0, MAX_EXPORTED_COLLECTIONS).map((item) => {
      const request = sanitizeRequestForPersistence(item.request);
      return {
        id: makeId(),
        name: name(item.name, "untitled"),
        folder: name(item.folder, ""),
        saved_at: Date.now(),
        request,
        requiresSecretReview: request.requiresSecretReview,
      };
    }),
  };
  try {
    const environments = parseEnvironmentExport(
      serializeEnvironmentExport({
        version: 1,
        environments: bundle.environments.slice(0, MAX_EXPORTED_ENVIRONMENTS).map((environment) => {
          if (environment.variables.length > MAX_EXPORTED_VARIABLES)
            warnings.push(`환경 변수가 많아 처음 ${MAX_EXPORTED_VARIABLES}개만 가져왔습니다.`);
          return {
            id: makeId(),
            name: name(environment.name, "untitled"),
            variables: environment.variables.slice(0, MAX_EXPORTED_VARIABLES).map((variable) => ({
              ...variable,
              secret: isSensitiveName(variable.key) || looksLikeSecret(variable.value),
            })),
          };
        }),
      }),
    );
    if (!environments) throw new ImportError("가져올 수 없는 형식입니다.");
    return { collections, environments, warnings };
  } catch {
    throw new ImportError("가져올 수 없는 형식입니다.");
  }
}
