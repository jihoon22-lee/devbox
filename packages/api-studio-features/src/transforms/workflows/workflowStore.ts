import { createDocumentSession, documentStorage, type DocumentStorage } from "../../storage/documentStorage";

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

export type RecentToolMetadata = import("../../generated/RecentToolMetadata").RecentToolMetadata;

export type SavedPipelineMetadata = import("../../generated/SavedPipelineMetadata").SavedPipelineMetadata;

export type WorkflowMetadata = import("../../generated/WorkflowMetadata").WorkflowMetadata;

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
  return (
    typeof value === "number" &&
    Number.isSafeInteger(value) &&
    value >= 0 &&
    value <= WORKFLOW_STORAGE_LIMITS.maxTimestamp
  );
}

function allowed(value: string, set: ReadonlySet<string> | undefined): boolean {
  return set === undefined || set.has(value);
}

function isPipelineStep(value: unknown): value is PipelineStep {
  return isRecord(value) && isSafeId(value.transformerId, WORKFLOW_STORAGE_LIMITS.maxToolIdLength);
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
        !isSafeId(toolId, WORKFLOW_STORAGE_LIMITS.maxToolIdLength) ||
        !allowed(toolId, options.toolIds) ||
        !safeTimestamp(entry.usedAt)
      )
        continue;
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
        !isSafeId(value, WORKFLOW_STORAGE_LIMITS.maxToolIdLength) ||
        !allowed(value, options.toolIds) ||
        favoriteSeen.has(value)
      )
        continue;
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
        !isSafeId(id, WORKFLOW_STORAGE_LIMITS.maxPipelineIdLength) ||
        !isPipelineValueType(entry.inputType) ||
        !safeTimestamp(entry.updatedAt) ||
        !Array.isArray(entry.steps) ||
        entry.steps.length === 0 ||
        entry.steps.length > PIPELINE_LIMITS.maxSteps
      )
        continue;
      const steps: PipelineStep[] = [];
      let valid = true;
      for (const rawStep of entry.steps) {
        if (!isRecord(rawStep) || !isSafeId(rawStep.transformerId, WORKFLOW_STORAGE_LIMITS.maxToolIdLength)) {
          valid = false;
          break;
        }
        if (
          (options.transformerIds !== undefined && !options.transformerIds.has(rawStep.transformerId)) ||
          !TRANSFORMER_BY_ID.has(rawStep.transformerId)
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
  if (!isSafeId(toolId, WORKFLOW_STORAGE_LIMITS.maxToolIdLength) || !allowed(toolId, toolIds) || !safeTimestamp(usedAt))
    return metadata;
  const recentTools = [{ toolId, usedAt }, ...metadata.recentTools.filter((entry) => entry.toolId !== toolId)]
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
    !isSafeId(id, WORKFLOW_STORAGE_LIMITS.maxPipelineIdLength) ||
    !isPipelineValueType(inputType) ||
    !Array.isArray(steps) ||
    steps.length === 0 ||
    steps.length > PIPELINE_LIMITS.maxSteps ||
    !safeTimestamp(updatedAt) ||
    !steps.every((step) => isPipelineStep(step) && TRANSFORMER_BY_ID.has(step.transformerId)) ||
    !validPipelineSteps(inputType, steps)
  )
    return metadata;
  const saved: SavedPipelineMetadata = {
    id,
    inputType,
    steps: steps.map((step) => ({ transformerId: step.transformerId })),
    updatedAt,
  };
  const pipelines = [saved, ...metadata.pipelines.filter((item) => item.id !== id)].slice(
    0,
    WORKFLOW_STORAGE_LIMITS.maxPipelines,
  );
  return { ...metadata, pipelines };
}

function fixedStorageError(): Error {
  return new Error(WORKFLOW_STORAGE_ERROR);
}

function hasStorageShape(value: unknown): value is Record<string, unknown> {
  return (
    isRecord(value) &&
    value.schemaVersion === WORKFLOW_SCHEMA_VERSION &&
    Array.isArray(value.recentTools) &&
    Array.isArray(value.favoriteTools) &&
    Array.isArray(value.pipelines)
  );
}

export interface WorkflowPersistence {
  load(): Promise<WorkflowMetadata>;
  save(metadata: WorkflowMetadata): Promise<void>;
}

export interface WorkflowPersistenceOptions extends WorkflowMetadataValidationOptions {
  readonly storage?: DocumentStorage;
}

/** Native app-local persistence with a safe browser-preview fallback. */
export function parseWorkflowDocument(body: string): WorkflowMetadata {
  if (new TextEncoder().encode(body).byteLength > WORKFLOW_STORAGE_LIMITS.maxSerializedBytes) throw fixedStorageError();
  const parsed: unknown = JSON.parse(body);
  if (!hasStorageShape(parsed)) throw fixedStorageError();
  return sanitizeWorkflowMetadata(parsed);
}

export function createWorkflowPersistence(options: WorkflowPersistenceOptions = {}): WorkflowPersistence {
  const session = createDocumentSession("workflows", options.storage ?? documentStorage());
  let saveChain = Promise.resolve();
  let writeBlocked = true;
  const load = async (): Promise<WorkflowMetadata> => {
    try {
      const document = await session.load();
      if (!document) {
        writeBlocked = false;
        return emptyMetadata();
      }
      if (new TextEncoder().encode(document.body).byteLength > WORKFLOW_STORAGE_LIMITS.maxSerializedBytes)
        throw fixedStorageError();
      const parsed: unknown = JSON.parse(document.body);
      if (!hasStorageShape(parsed)) throw fixedStorageError();
      writeBlocked = false;
      return sanitizeWorkflowMetadata(parsed, options);
    } catch {
      writeBlocked = true;
      throw fixedStorageError();
    }
  };
  const save = (metadata: WorkflowMetadata): Promise<void> => {
    if (writeBlocked) return Promise.reject(fixedStorageError());
    const serialized = serializeWorkflowMetadata(metadata, options);
    const action = saveChain.then(async () => {
      if (writeBlocked) throw fixedStorageError();
      await session.save(serialized);
    });
    saveChain = action.catch(() => {
      writeBlocked = true;
    });
    return action;
  };
  return { load, save };
}

export { emptyMetadata };
