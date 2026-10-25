import { ChangeSet, Text } from "@codemirror/state";
import type { RpcClient } from "../../../rpc/client";
import type { DocKey } from "../../../rpc/gen/rpc";
import { type Doc, sameRev } from "../../documents/machine";
import { keyId } from "../../documents/offlineJournal";
import { useDocuments } from "../../documents/store";
import { useSource } from "../store";
import { validCodeKey } from "./location";
import {
  proofIdentity,
  textHash,
  type WorkspaceIdentity,
  type WorkspaceProof,
  type WorkspaceValidation,
} from "./lspProof";

export function projectUris(projectId: string, path: string) {
  const base = path.replace(/\/+$/, "");
  if (!path.startsWith("/") || base.includes("\0")) throw new Error("프로젝트 경로가 올바르지 않습니다.");
  const root = `project:${projectId}`;
  // Match daemon uri::file_uri: only ASCII alphanumerics and /-._~ stay literal.
  const encode = (value: string) =>
    `file://${value
      .split("/")
      .map((part) =>
        encodeURIComponent(part).replace(/[!'()*]/g, (char) => `%${char.charCodeAt(0).toString(16).toUpperCase()}`),
      )
      .join("/")}`;
  const uri = (key: DocKey) => {
    if (key.root !== root || !validCodeKey(key)) throw new Error("프로젝트 밖 파일입니다.");
    return encode(`${base}/${key.path}`);
  };
  const key = (value: string): DocKey => {
    const parsed = new URL(value);
    const decoded = decodeURIComponent(parsed.pathname);
    if (parsed.protocol !== "file:" || parsed.host || parsed.search || parsed.hash || !decoded.startsWith(`${base}/`))
      throw new Error("프로젝트 밖 파일입니다.");
    const result = { root, path: decoded.slice(base.length + 1) };
    if (uri(result) !== parsed.href) throw new Error("파일 URI가 올바르지 않습니다.");
    return result;
  };
  return { uri, key, rootUri: encode(base || "/"), root };
}
export type ProjectUris = ReturnType<typeof projectUris>;
export interface EditContext {
  rpc: Pick<RpcClient, "call">;
  uris: ProjectUris;
  current(): boolean;
  /** Snapshot at request time; local edits while awaiting a response reject the whole change. */
  expected?: ReadonlyMap<string, Doc>;
  version?(uri: string): number | undefined;
  proof?: WorkspaceProof;
  validate?(request: WorkspaceValidation): Promise<boolean>;
  finish?(identity: WorkspaceIdentity): void;
  /** Unsolicited requests require an exact owned relay version. */
  unsolicited?: boolean;
}
interface Position {
  line: number;
  character: number;
}
interface Edit {
  range: { start: Position; end: Position };
  newText: string;
}
const refusal = () =>
  new Error(
    "파일 상태가 바뀌었거나 변경 범위를 안전하게 확인하지 못했습니다. 범위를 줄여 이름 바꾸기를 다시 요청하거나 변경을 직접 검토해 주세요.",
  );
function record(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw refusal();
  return value as Record<string, unknown>;
}
function entries(value: unknown): { uri: string; edits: unknown; version?: number }[] {
  const edit = record(value);
  if (edit.changeAnnotations !== undefined || (edit.changes !== undefined && edit.documentChanges !== undefined))
    throw refusal();
  if (edit.changes !== undefined) return Object.entries(record(edit.changes)).map(([uri, edits]) => ({ uri, edits }));
  if (edit.documentChanges !== undefined) {
    if (!Array.isArray(edit.documentChanges)) throw refusal();
    return edit.documentChanges.map((item) => {
      const row = record(item),
        doc = record(row.textDocument);
      if (
        row.kind !== undefined ||
        typeof doc.uri !== "string" ||
        (doc.version != null && !Number.isSafeInteger(doc.version))
      )
        throw refusal();
      return { uri: doc.uri, edits: row.edits, ...(doc.version != null ? { version: doc.version as number } : {}) };
    });
  }
  return [];
}
function position(text: Text, value: unknown): number {
  const pos = record(value);
  if (!Number.isSafeInteger(pos.line) || !Number.isSafeInteger(pos.character)) throw refusal();
  const { line, character } = pos as unknown as Position;
  if (line < 0 || line >= text.lines || character < 0) throw refusal();
  const row = text.line(line + 1);
  if (character > row.length) throw refusal();
  // Never split a surrogate pair even though offsets are UTF-16 code units.
  if (
    character > 0 &&
    /[\uD800-\uDBFF]/.test(row.text[character - 1]!) &&
    /[\uDC00-\uDFFF]/.test(row.text[character] ?? "")
  )
    throw refusal();
  return row.from + character;
}
function replacement(original: string, edits: unknown): string {
  if (!Array.isArray(edits) || edits.length > 10_000) throw refusal();
  const text = Text.of(original.split("\n"));
  const changes = edits
    .map((item) => {
      const edit = record(item),
        range = record(edit.range);
      if (typeof edit.newText !== "string" || edit.annotationId !== undefined) throw refusal();
      const from = position(text, range.start),
        to = position(text, range.end);
      if (to < from) throw refusal();
      return { from, to, insert: edit.newText };
    })
    .sort((a, b) => a.from - b.from || a.to - b.to);
  for (let i = 1; i < changes.length; i++) if (changes[i]!.from < changes[i - 1]!.to) throw refusal();
  return ChangeSet.of(changes, text.length).apply(text).toString();
}
function writable(doc: Doc | undefined): doc is Doc & { leaseId: string } {
  return (
    !!doc?.leaseId &&
    doc.policy.kind === "code" &&
    (doc.state === "Clean" || doc.state === "Dirty") &&
    !doc.diskReadOnly
  );
}

/** Validate every target before one synchronous, undoable buffer-only commit. */
export async function applyWorkspaceEdit(context: EditContext, value: unknown): Promise<void> {
  try {
    await apply(context, value);
  } finally {
    if (context.proof) context.finish?.(proofIdentity(context.proof));
  }
}
async function apply(context: EditContext, value: unknown): Promise<void> {
  if (!context.current()) throw refusal();
  const rows = entries(value);
  if (rows.length > 256) throw refusal();
  const targets = rows.map((row) => ({ ...row, key: context.uris.key(row.uri) }));
  if (new Set(targets.map(({ key }) => keyId(key))).size !== targets.length) throw refusal();
  const proof = context.proof;
  if (proof && (proof.rootUri !== context.uris.rootUri || !context.validate || !context.finish)) throw refusal();
  const baseline = new Map(proof?.targets.map((target) => [target.uri, target]));
  if (proof && (baseline.size !== targets.length || proof.targets.length !== targets.length)) throw refusal();
  const store = useDocuments.getState();
  const newlyOpened = targets.filter(({ key }) => !store.find(key)).map(({ key }) => key);
  for (const { key } of targets) store.open(key, { kind: "code" });
  await store.settled();
  const plans = targets.map(({ key, uri, edits, version }) => {
    const doc = store.find(key);
    if (!writable(doc) || (version !== undefined && version !== context.version?.(uri))) throw refusal();
    const expected = context.expected?.get(keyId(key));
    if (context.unsolicited && (version === undefined || !expected)) throw refusal();
    if (!expected && !baseline.has(uri)) throw refusal();
    if (proof && !baseline.has(uri)) throw refusal();
    if (
      expected &&
      (expected.text !== doc.text ||
        expected.editSeq !== doc.editSeq ||
        expected.leaseId !== doc.leaseId ||
        !sameRev(expected.diskRev, doc.diskRev))
    )
      throw refusal();
    return { doc, uri, text: replacement(doc.text, edits) };
  });
  const validated = await Promise.all(
    plans.map(async ({ doc, uri }) => {
      const expected = baseline.get(uri);
      const hash = await textHash(doc.text);
      if (
        expected &&
        (expected.textHash !== hash ||
          !exactRev(expected.diskRev, doc.diskRev) ||
          (expected.leaseId !== undefined &&
            (expected.leaseId !== doc.leaseId || expected.version !== context.version?.(uri))))
      )
        throw refusal();
      return {
        uri,
        diskRev: doc.diskRev ?? null,
        textHash: hash,
        leaseId: doc.leaseId,
        ...(expected?.version !== undefined ? { version: expected.version } : {}),
      };
    }),
  );
  const { lost } = await context.rpc.call("documents.renew", { leaseIds: plans.map(({ doc }) => doc.leaseId) });
  if (lost.length) throw refusal();
  for (const { doc } of proof ? [] : plans) {
    const disk = await context.rpc.call("documents.read", { key: doc.key, policy: { kind: "code" } });
    if (disk.readOnly || !sameRev(disk.rev, doc.diskRev)) throw refusal();
  }
  if (
    proof &&
    !(await context.validate?.({
      ...proofIdentity(proof),
      targets: validated.sort((a, b) => (a.uri < b.uri ? -1 : a.uri > b.uri ? 1 : 0)),
    }))
  )
    throw refusal();
  const current = useDocuments.getState();
  if (
    !context.current() ||
    current.windowClosing ||
    plans.some(({ doc }) => current.closingDocs[doc.id] || current.docs[doc.id] !== doc)
  )
    throw refusal();
  // No awaited work after this point. Store/editor subscribers run synchronously.
  for (const { doc, text } of plans) current.replace(doc.key, text);
  for (const key of newlyOpened) useSource.getState().send({ type: "open", key, pinned: true });
}

export type TextEdit = Edit;

function exactRev(
  a: import("../../../rpc/gen/rpc").DiskRev | null | undefined,
  b: import("../../../rpc/gen/rpc").DiskRev | null | undefined,
): boolean {
  return (
    a === b || (!!a && !!b && a.hash === b.hash && a.size === b.size && a.ino === b.ino && a.mtimeNs === b.mtimeNs)
  );
}
