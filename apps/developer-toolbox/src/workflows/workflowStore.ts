import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "../lib/isTauri";
import {
  PIPELINE_LIMITS,
  isPipelineValueType,
  TRANSFORMER_BY_ID,
  type PipelineStep,
  type PipelineValueType,
} from "./transformPipeline";

export const WORKFLOW_SCHEMA_VERSION = 1 as const;
export const WORKFLOW_STORAGE_KEY = "devbox.developer-toolbox.smart-workflows.v1";

export const WORKFLOW_STORAGE_LIMITS = Object.freeze({
  maxRecentTools: 20,
  maxFavoriteTools: 50,
  maxPipelines: 20,
  maxPipelineIdLength: 64,
  maxToolIdLength: 64,
  maxTimestamp: Number.MAX_SAFE_INTEGER,
  maxSerializedBytes: 64 * 1024,
});

export interface RecentToolMetadata {
  readonly toolId: string;
  readonly usedAt: number;
}

export interface SavedPipelineMetadata {
  readonly id: string;
  readonly inputType: PipelineValueType;
  readonly steps: readonly PipelineStep[];
  readonly updatedAt: number;
}

export interface WorkflowMetadata {
  readonly schemaVersion: typeof WORKFLOW_SCHEMA_VERSION;
  readonly recentTools: readonly RecentToolMetadata[];
  readonly favoriteTools: readonly string[];
  readonly pipelines: readonly SavedPipelineMetadata[];
}

export const WORKFLOW_STORAGE_ERROR = "Toolbox 워크플로 메타데이터를 저장하거나 읽지 못했습니다.";

const ID = /^[a-z0-9]+(?:-[a-z0-9]+)*$/u;

function emptyMetadata(): WorkflowMetadata {
  return {
    schemaVersion: WORKFLOW_SCHEMA_VERSION,
    recentTools: [],
    favoriteTools: [],
    pipelines: [],
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isSafeId(value: unknown, maxLength: number): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= maxLength && ID.test(value);
}

function compareIds(left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0;
}

function safeTimestamp(value: unknown): value is number {
  return typeof value === "number"
    && Number.isSafeInteger(value)
    && value >= 0
    && value <= WORKFLOW_STORAGE_LIMITS.maxTimestamp;
}

function allowed(value: string, set: ReadonlySet<string> | undefined): boolean {
  return set === undefined || set.has(value);
}

function isPipelineStep(value: unknown): value is PipelineStep {
  return isRecord(value)
    && isSafeId(value.transformerId, WORKFLOW_STORAGE_LIMITS.maxToolIdLength);
}

function validPipelineSteps(inputType: PipelineValueType, steps: readonly PipelineStep[]): boolean {
  if (!isPipelineValueType(inputType) || !Array.isArray(steps)) return false;
  let currentType = inputType;
  for (const step of steps) {
    if (!isPipelineStep(step)) return false;
    const transformer = TRANSFORMER_BY_ID.get(step.transformerId);
    if (!transformer || !transformer.inputTypes.includes(currentType)) return false;
    currentType = transformer.outputType;
  }
  return true;
}

export interface WorkflowMetadataValidationOptions {
  readonly toolIds?: ReadonlySet<string>;
  readonly transformerIds?: ReadonlySet<string>;
}

/**
 * Rebuild metadata from an allow-listed shape.  Unknown fields (including
 * input/output/raw/secret-looking additions) are intentionally ignored.
 * Invalid entries are discarded instead of being shown or written back.
 */
export function sanitizeWorkflowMetadata(
  raw: unknown,
  options: WorkflowMetadataValidationOptions = {},
): WorkflowMetadata {
  if (!isRecord(raw) || raw.schemaVersion !== WORKFLOW_SCHEMA_VERSION) return emptyMetadata();

  const recentById = new Map<string, number>();
  if (Array.isArray(raw.recentTools)) {
    for (const entry of raw.recentTools) {
      if (!isRecord(entry)) continue;
      const toolId = entry.toolId;
      if (
        !isSafeId(toolId, WORKFLOW_STORAGE_LIMITS.maxToolIdLength)
        || !allowed(toolId, options.toolIds)
        || !safeTimestamp(entry.usedAt)
      ) continue;
      const previous = recentById.get(toolId);
      if (previous === undefined || entry.usedAt > previous) recentById.set(toolId, entry.usedAt);
    }
  }
  const recentTools: RecentToolMetadata[] = [...recentById.entries()]
    .map(([toolId, usedAt]) => ({ toolId, usedAt }))
    .sort((left, right) => right.usedAt - left.usedAt || compareIds(left.toolId, right.toolId))
    .slice(0, WORKFLOW_STORAGE_LIMITS.maxRecentTools);

  const favoriteTools: string[] = [];
  const favoriteSeen = new Set<string>();
  if (Array.isArray(raw.favoriteTools)) {
    for (const value of raw.favoriteTools) {
      if (
        !isSafeId(value, WORKFLOW_STORAGE_LIMITS.maxToolIdLength)
        || !allowed(value, options.toolIds)
        || favoriteSeen.has(value)
      ) continue;
      favoriteSeen.add(value);
      favoriteTools.push(value);
      if (favoriteTools.length >= WORKFLOW_STORAGE_LIMITS.maxFavoriteTools) break;
    }
  }

  const pipelineById = new Map<string, SavedPipelineMetadata>();
  if (Array.isArray(raw.pipelines)) {
    for (const entry of raw.pipelines) {
      if (!isRecord(entry)) continue;
      const id = entry.id;
      if (
        !isSafeId(id, WORKFLOW_STORAGE_LIMITS.maxPipelineIdLength)
        || !isPipelineValueType(entry.inputType)
        || !safeTimestamp(entry.updatedAt)
        || !Array.isArray(entry.steps)
        || entry.steps.length === 0
        || entry.steps.length > PIPELINE_LIMITS.maxSteps
      ) continue;
      const steps: PipelineStep[] = [];
      let valid = true;
      for (const rawStep of entry.steps) {
        if (!isRecord(rawStep) || !isSafeId(rawStep.transformerId, WORKFLOW_STORAGE_LIMITS.maxToolIdLength)) {
          valid = false;
          break;
        }
        if (
          (options.transformerIds !== undefined && !options.transformerIds.has(rawStep.transformerId))
          || !TRANSFORMER_BY_ID.has(rawStep.transformerId)
        ) {
          valid = false;
          break;
        }
        steps.push({ transformerId: rawStep.transformerId });
      }
      if (!valid) continue;
      if (!validPipelineSteps(entry.inputType as PipelineValueType, steps)) continue;
      const saved = { id, inputType: entry.inputType, steps, updatedAt: entry.updatedAt };
      const previous = pipelineById.get(id);
      if (previous === undefined || saved.updatedAt > previous.updatedAt) pipelineById.set(id, saved);
    }
  }
  const pipelines: SavedPipelineMetadata[] = [...pipelineById.values()]
    .sort((left, right) => right.updatedAt - left.updatedAt || compareIds(left.id, right.id))
    .slice(0, WORKFLOW_STORAGE_LIMITS.maxPipelines);

  return { schemaVersion: WORKFLOW_SCHEMA_VERSION, recentTools, favoriteTools, pipelines };
}

export function serializeWorkflowMetadata(
  metadata: WorkflowMetadata,
  options: WorkflowMetadataValidationOptions = {},
): string {
  const safe = sanitizeWorkflowMetadata(metadata, options);
  let serialized: string;
  try {
    serialized = JSON.stringify(safe);
  } catch {
    throw new Error(WORKFLOW_STORAGE_ERROR);
  }
  if (new TextEncoder().encode(serialized).byteLength > WORKFLOW_STORAGE_LIMITS.maxSerializedBytes) {
    throw new Error(WORKFLOW_STORAGE_ERROR);
  }
  return serialized;
}

export function recordRecentTool(
  metadata: WorkflowMetadata,
  toolId: string,
  usedAt: number,
  toolIds?: ReadonlySet<string>,
): WorkflowMetadata {
  if (
    !isSafeId(toolId, WORKFLOW_STORAGE_LIMITS.maxToolIdLength)
    || !allowed(toolId, toolIds)
    || !safeTimestamp(usedAt)
  ) return metadata;
  const recentTools = [
    { toolId, usedAt },
    ...metadata.recentTools.filter((entry) => entry.toolId !== toolId),
  ]
    .sort((left, right) => right.usedAt - left.usedAt || compareIds(left.toolId, right.toolId))
    .slice(0, WORKFLOW_STORAGE_LIMITS.maxRecentTools);
  return { ...metadata, recentTools };
}

export function toggleFavoriteTool(
  metadata: WorkflowMetadata,
  toolId: string,
  toolIds?: ReadonlySet<string>,
): WorkflowMetadata {
  if (!isSafeId(toolId, WORKFLOW_STORAGE_LIMITS.maxToolIdLength) || !allowed(toolId, toolIds)) return metadata;
  const favoriteTools = metadata.favoriteTools.includes(toolId)
    ? metadata.favoriteTools.filter((id) => id !== toolId)
    : [...metadata.favoriteTools, toolId].slice(0, WORKFLOW_STORAGE_LIMITS.maxFavoriteTools);
  return { ...metadata, favoriteTools };
}

export function nextPipelineId(metadata: WorkflowMetadata): string | null {
  const used = new Set(metadata.pipelines.map((item) => item.id));
  for (let index = 1; index <= WORKFLOW_STORAGE_LIMITS.maxPipelines; index += 1) {
    const id = `pipeline-${index}`;
    if (!used.has(id)) return id;
  }
  return null;
}

export function upsertPipeline(
  metadata: WorkflowMetadata,
  id: string,
  inputType: PipelineValueType,
  steps: readonly PipelineStep[],
  updatedAt: number,
): WorkflowMetadata {
  if (
    !isSafeId(id, WORKFLOW_STORAGE_LIMITS.maxPipelineIdLength)
    || !isPipelineValueType(inputType)
    || !Array.isArray(steps)
    || steps.length === 0
    || steps.length > PIPELINE_LIMITS.maxSteps
    || !safeTimestamp(updatedAt)
    || !steps.every((step) => isPipelineStep(step) && TRANSFORMER_BY_ID.has(step.transformerId))
    || !validPipelineSteps(inputType, steps)
  ) return metadata;
  const saved: SavedPipelineMetadata = {
    id,
    inputType,
    steps: steps.map((step) => ({ transformerId: step.transformerId })),
    updatedAt,
  };
  const pipelines = [saved, ...metadata.pipelines.filter((item) => item.id !== id)]
    .slice(0, WORKFLOW_STORAGE_LIMITS.maxPipelines);
  return { ...metadata, pipelines };
}

function fixedStorageError(): Error {
  return new Error(WORKFLOW_STORAGE_ERROR);
}

function hasStorageShape(value: unknown): value is Record<string, unknown> {
  return isRecord(value)
    && value.schemaVersion === WORKFLOW_SCHEMA_VERSION
    && Array.isArray(value.recentTools)
    && Array.isArray(value.favoriteTools)
    && Array.isArray(value.pipelines);
}

export interface WorkflowPersistence {
  load(): Promise<WorkflowMetadata>;
  save(metadata: WorkflowMetadata): Promise<void>;
}

export interface WorkflowPersistenceOptions extends WorkflowMetadataValidationOptions {
  readonly storage?: Storage;
}

/** Native app-local persistence with a safe browser-preview fallback. */
export function createWorkflowPersistence(options: WorkflowPersistenceOptions = {}): WorkflowPersistence {
  const storage = options.storage;
  let saveChain = Promise.resolve();
  let writeBlocked = false;

  const load = async (): Promise<WorkflowMetadata> => {
    if (isTauri()) {
      try {
        const raw = await invoke<unknown>("load_workflow_metadata");
        if (isRecord(raw) && "metadata" in raw && typeof raw.writable === "boolean") {
          writeBlocked = !raw.writable;
          if (!raw.writable || !hasStorageShape(raw.metadata)) throw fixedStorageError();
          return sanitizeWorkflowMetadata(raw.metadata, options);
        }
        // Keep browser-preview mocks and older development builds readable.
        writeBlocked = false;
        return sanitizeWorkflowMetadata(raw, options);
      } catch {
        writeBlocked = true;
        throw fixedStorageError();
      }
    }
    try {
      const source = storage ?? (typeof window !== "undefined" ? window.localStorage : undefined);
      if (!source) {
        writeBlocked = false;
        return emptyMetadata();
      }
      const raw = source.getItem(WORKFLOW_STORAGE_KEY);
      if (raw === null) {
        writeBlocked = false;
        return emptyMetadata();
      }
      if (new TextEncoder().encode(raw).byteLength > WORKFLOW_STORAGE_LIMITS.maxSerializedBytes) {
        writeBlocked = true;
        throw fixedStorageError();
      }
      let parsed: unknown;
      try {
        parsed = JSON.parse(raw) as unknown;
      } catch {
        writeBlocked = true;
        throw fixedStorageError();
      }
      if (!hasStorageShape(parsed)) {
        writeBlocked = true;
        throw fixedStorageError();
      }
      writeBlocked = false;
      return sanitizeWorkflowMetadata(parsed, options);
    } catch {
      writeBlocked = true;
      throw fixedStorageError();
    }
  };

  const save = (metadata: WorkflowMetadata): Promise<void> => {
    if (writeBlocked) return Promise.reject(fixedStorageError());
    const serialized = (() => {
      try {
        return serializeWorkflowMetadata(metadata, options);
      } catch {
        throw fixedStorageError();
      }
    })();

    const action = saveChain.then(async () => {
      if (writeBlocked) throw fixedStorageError();
      if (isTauri()) {
        try {
          await invoke("save_workflow_metadata", {
            // Send the already-bounded JSON string so the native command can
            // reject an oversized IPC payload before deserializing vectors.
            serializedMetadata: serialized,
          });
          return;
        } catch {
          throw fixedStorageError();
        }
      }
      try {
        const target = storage ?? (typeof window !== "undefined" ? window.localStorage : undefined);
        if (!target) return;
        target.setItem(WORKFLOW_STORAGE_KEY, serialized);
      } catch {
        throw fixedStorageError();
      }
    });
    // A failed save must not poison later explicit user saves.
    saveChain = action.catch(() => undefined);
    return action;
  };

  return { load, save };
}

export { emptyMetadata };
