import {
  documentStorage,
  blockFailedMigrations,
  storageError,
  type DocumentStorage,
  type DocumentKind,
} from "./documentStorage";
import { LEGACY_KEYS, migrateLocalDocuments } from "./migrate";
import { parseStore as parseEnvironments, emptyStore as emptyEnvironments } from "../requests/lib/environments";
import { parseStore as parseCollections, sanitizeStore as sanitizeCollections } from "../requests/lib/collections";
import { parseHistoryStore, sanitizeHistoryStore } from "../requests/lib/persistence";
import { parseGrpcHistory } from "../requests/lib/grpc";
import { parseWorkflowDocument } from "../transforms/workflows/workflowStore";
import { sanitizePersistedJson } from "../requests/api";

type Result = { migrated: DocumentKind[]; failed: DocumentKind[] };
const starts = new WeakMap<DocumentStorage, Promise<Result>>();
/** Once per product window. A failed source stays quarantined until the next launch. */
export function initializeStudioDocuments(
  target: DocumentStorage = documentStorage(),
  source: Storage = localStorage,
): Promise<Result> {
  const previous = starts.get(target);
  if (previous) return previous;
  let environments: Promise<ReturnType<typeof emptyEnvironments>> | undefined;
  let knownVariables: import("../requests/lib/environments").EnvVariable[] = [];
  const environment = () =>
    (environments ??= (async () => {
      const legacy = source.getItem(LEGACY_KEYS.environments);
      const current = await target.load("environments");
      const parsed = legacy === null ? emptyEnvironments() : parseEnvironments(legacy);
      const native = current ? parseEnvironments(current.body) : emptyEnvironments();
      if (!parsed || !native) throw storageError("store_document_invalid");
      // Validate every sealed value without materializing plaintext in the renderer.
      knownVariables = [...parsed.environments, ...native.environments].flatMap((entry) => entry.variables);
      await sanitizePersistedJson("{}", knownVariables);
      return parsed;
    })());
  const prepare = async (kind: DocumentKind, body: string): Promise<string> => {
    if (kind === "grpc_history") {
      const parsed = parseGrpcHistory(body);
      if (!parsed) throw storageError("store_document_invalid");
      return JSON.stringify(parsed);
    }
    if (kind === "workflows") return JSON.stringify(parseWorkflowDocument(body));
    await environment();
    if (kind === "environments") {
      const parsed = parseEnvironments(body);
      if (!parsed) throw storageError("store_document_invalid");
      return JSON.stringify(parsed);
    }
    const variables = knownVariables;
    const sanitize = (serialized: string) => sanitizePersistedJson(serialized, variables);
    if (kind === "collections") {
      const parsed = parseCollections(body);
      if (!parsed) throw storageError("store_document_invalid");
      return JSON.stringify(await sanitizeCollections(parsed, sanitize));
    }
    const parsed = parseHistoryStore(body);
    if (!parsed) throw storageError("store_document_invalid");
    return JSON.stringify(await sanitizeHistoryStore(parsed, sanitize));
  };
  const start = migrateLocalDocuments(target, source, prepare).then((result) => {
    blockFailedMigrations(result.failed);
    return result;
  });
  starts.set(target, start);
  void start.then(
    () => starts.delete(target),
    () => starts.delete(target),
  );
  return start;
}
