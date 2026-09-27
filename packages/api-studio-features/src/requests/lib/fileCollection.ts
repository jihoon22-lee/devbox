import type { CollectionStore } from "./collections";
import {
  parseCollectionExport,
  serializeCollectionExport,
  COLLECTION_EXPORT_SCHEMA,
  looksLikeSecret,
} from "./transfer";
import { ImportError, type ImportBundle } from "./importers/model";
import { parseBruno } from "./importers/bruno";
export const FILE_COLLECTION_MARKER = "collection.devbox.json";
const schema = "devbox.api-studio.file-collection";
const requestSchema = "devbox.api-studio.request";
const invalid = () => new ImportError("가져올 수 없는 형식입니다.");
export function safeFileName(name: string): string {
  let value = name
    .normalize("NFC")
    .replace(/[<>:"/\\|?*\u0000-\u001f\u007f]/g, "-")
    .trim()
    .slice(0, 80)
    .replace(/[. ]+$/, "");
  if (/^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\.|$)/i.test(value)) value = `_${value}`.slice(0, 80);
  return value || "untitled";
}
export function serializeFileCollection(
  name: string,
  store: CollectionStore,
): { relativePath: string; text: string }[] {
  const clean = parseCollectionExport(serializeCollectionExport(store));
  if (!clean) throw invalid();
  const json = (value: unknown) => `${JSON.stringify(value, null, 2)}\n`;
  const files = [
    {
      relativePath: FILE_COLLECTION_MARKER,
      text: json({ schema, schemaVersion: 1, name: looksLikeSecret(name) ? "Collection" : safeFileName(name) }),
    },
  ];
  const used = new Set([FILE_COLLECTION_MARKER.toLowerCase()]);
  for (const item of clean.collections) {
    const folders = item.folder ? item.folder.split("/").map(safeFileName) : [];
    if (folders.length > 8) throw invalid();
    const base = safeFileName(item.name);
    let sequence = 1;
    let relativePath: string;
    do {
      const suffix = `${sequence > 1 ? ` (${sequence})` : ""}.request.json`;
      const characters = Array.from(base);
      while (new TextEncoder().encode(characters.join("") + suffix).length > 255) characters.pop();
      relativePath = [...folders, characters.join("") + suffix].join("/");
      sequence++;
    } while (used.has(relativePath.toLowerCase()));
    used.add(relativePath.toLowerCase());
    const text = json({
      schema: requestSchema,
      schemaVersion: 1,
      name: item.name,
      request: {
        ...item.request,
        requiresSecretReview: item.requiresSecretReview || item.request.requiresSecretReview,
      },
    });
    if (new TextEncoder().encode(text).length > 1024 * 1024) throw invalid();
    files.push({ relativePath, text });
  }
  return files;
}
export function parseFileCollection(files: { relativePath: string; text: string }[]): ImportBundle {
  if (!files.length || files.length > 1000) throw invalid();
  const bundle: ImportBundle = { requests: [], environments: [], warnings: [] };
  const bruno: typeof files = [];
  const used = new Set<string>();
  for (const file of files) {
    const parts = file.relativePath.split("/");
    if (
      parts.length > 9 ||
      parts.some((part) => !part || part === "." || part === ".." || /[\\:\u0000-\u001f]/.test(part)) ||
      used.has(file.relativePath.toLowerCase()) ||
      new TextEncoder().encode(file.text).length > 1024 * 1024
    )
      throw invalid();
    used.add(file.relativePath.toLowerCase());
    if (/\.bru$/i.test(file.relativePath)) {
      bruno.push(file);
      continue;
    }
    try {
      const value = JSON.parse(file.text.replace(/^\uFEFF/, ""));
      if (file.relativePath === FILE_COLLECTION_MARKER) {
        if (value.schema !== schema || value.schemaVersion !== 1) throw invalid();
        continue;
      }
      if (
        !file.relativePath.endsWith(".request.json") ||
        value.schema !== requestSchema ||
        value.schemaVersion !== 1 ||
        typeof value.name !== "string"
      )
        throw invalid();
      const parsed = parseCollectionExport(
        JSON.stringify({
          schema: COLLECTION_EXPORT_SCHEMA,
          schema_version: 1,
          collections: [
            {
              id: "import",
              name: value.name,
              folder: parts.slice(0, -1).join("/"),
              saved_at: 0,
              request: value.request,
              requiresSecretReview: value.request?.requiresSecretReview === true,
            },
          ],
        }),
      );
      const item = parsed?.collections[0];
      if (!item) throw invalid();
      bundle.requests.push({
        name: item.name,
        folder: item.folder,
        request: item.request,
        requiresSecretReview: item.requiresSecretReview || item.request.requiresSecretReview,
      });
    } catch {
      throw invalid();
    }
  }
  if (bruno.length) {
    const parsed = parseBruno(bruno);
    bundle.requests.push(...parsed.requests);
    bundle.environments.push(...parsed.environments);
    bundle.warnings.push(...parsed.warnings);
  }
  return bundle;
}
