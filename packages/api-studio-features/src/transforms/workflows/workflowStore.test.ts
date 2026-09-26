import { MemoryDocuments as MemoryStorage } from "../../storage/testDocuments";
import { describe, expect, it } from "vitest";
import {
  createWorkflowPersistence,
  emptyMetadata,
  nextPipelineId,
  recordRecentTool,
  sanitizeWorkflowMetadata,
  serializeWorkflowMetadata,
  toggleFavoriteTool,
  upsertPipeline,
  WORKFLOW_SCHEMA_VERSION,
  WORKFLOW_STORAGE_ERROR,
  WORKFLOW_STORAGE_LIMITS,
} from "./workflowStore";

const toolIds = new Set(["json-format", "byte-codec", "jwt"]);
const transformerIds = new Set(["json-format", "base64-decode", "json-to-typescript"]);

describe("workflow metadata persistence (#342)", () => {
  it("stores only bounded IDs and timestamps, dropping raw fields and unknown entries", () => {
    const raw = {
      schemaVersion: WORKFLOW_SCHEMA_VERSION,
      input: "credential-value",
      output: "private-result",
      recentTools: [
        { toolId: "json-format", usedAt: 20 },
        { toolId: "../secret", usedAt: 19 },
      ],
      favoriteTools: ["byte-codec", "unknown-tool"],
      pipelines: [
        {
          id: "pipeline-1",
          inputType: "base64",
          steps: [{ transformerId: "base64-decode" }],
          updatedAt: 30,
          input: "do-not-store",
        },
        {
          id: "pipeline-2",
          inputType: "text",
          steps: [{ transformerId: "run-shell" }],
          updatedAt: 29,
        },
      ],
    };

    const safe = sanitizeWorkflowMetadata(raw, { toolIds, transformerIds });
    const serialized = serializeWorkflowMetadata(safe);

    expect(safe.recentTools).toEqual([{ toolId: "json-format", usedAt: 20 }]);
    expect(safe.favoriteTools).toEqual(["byte-codec"]);
    expect(safe.pipelines).toHaveLength(1);
    expect(serialized).not.toContain("credential-value");
    expect(serialized).not.toContain("private-result");
    expect(serialized).not.toContain("do-not-store");
    expect(serialized).not.toContain("run-shell");
  });

  it("bounds and reorders recent tools and toggles favorites without text input", () => {
    let metadata = emptyMetadata();
    for (let index = 0; index < WORKFLOW_STORAGE_LIMITS.maxRecentTools + 3; index += 1) {
      metadata = recordRecentTool(metadata, `tool-${index}`, index + 1);
    }
    metadata = recordRecentTool(metadata, "tool-2", 10_000);
    metadata = toggleFavoriteTool(metadata, "json-format", toolIds);
    metadata = toggleFavoriteTool(metadata, "json-format", toolIds);
    metadata = toggleFavoriteTool(metadata, "byte-codec", toolIds);

    expect(metadata.recentTools).toHaveLength(WORKFLOW_STORAGE_LIMITS.maxRecentTools);
    expect(metadata.recentTools[0]).toEqual({ toolId: "tool-2", usedAt: 10_000 });
    expect(metadata.favoriteTools).toEqual(["byte-codec"]);
  });

  it("keeps recent tools ordered by timestamp even when events arrive out of order", () => {
    let metadata = emptyMetadata();
    metadata = recordRecentTool(metadata, "json-format", 200);
    metadata = recordRecentTool(metadata, "byte-codec", 100);

    expect(metadata.recentTools).toEqual([
      { toolId: "json-format", usedAt: 200 },
      { toolId: "byte-codec", usedAt: 100 },
    ]);
  });

  it("retains the newest valid duplicate and newest entries when metadata exceeds the cap", () => {
    const recentTools = [
      { toolId: "json-format", usedAt: 1 },
      ...Array.from({ length: WORKFLOW_STORAGE_LIMITS.maxRecentTools }, (_, index) => ({
        toolId: `tool-${index}`,
        usedAt: index + 2,
      })),
      { toolId: "json-format", usedAt: 10_000 },
    ];
    const safe = sanitizeWorkflowMetadata({
      schemaVersion: WORKFLOW_SCHEMA_VERSION,
      recentTools,
      favoriteTools: [],
      pipelines: [],
    });

    expect(safe.recentTools).toHaveLength(WORKFLOW_STORAGE_LIMITS.maxRecentTools);
    expect(safe.recentTools[0]).toEqual({ toolId: "json-format", usedAt: 10_000 });
  });

  it("can apply the known tool allow-list during serialization", () => {
    const unsafe = {
      ...emptyMetadata(),
      favoriteTools: ["not-a-tool"],
    } as never;

    expect(serializeWorkflowMetadata(unsafe, { toolIds })).not.toContain("not-a-tool");
  });

  it("drops pipelines whose persisted stages do not connect by type", () => {
    const raw = {
      schemaVersion: WORKFLOW_SCHEMA_VERSION,
      recentTools: [],
      favoriteTools: [],
      pipelines: [
        {
          id: "pipeline-1",
          inputType: "text",
          steps: [{ transformerId: "json-format" }],
          updatedAt: 1,
        },
      ],
    };

    expect(sanitizeWorkflowMetadata(raw, { toolIds, transformerIds }).pipelines).toEqual([]);
    expect(upsertPipeline(emptyMetadata(), "pipeline-1", "text", [{ transformerId: "json-format" }], 1)).toEqual(
      emptyMetadata(),
    );
    expect(() => upsertPipeline(emptyMetadata(), "pipeline-1", "text", [null as never], 1)).not.toThrow();
  });

  it("does not overwrite an unrelated pipeline when the library is full", () => {
    let metadata = emptyMetadata();
    for (let index = 1; index <= WORKFLOW_STORAGE_LIMITS.maxPipelines; index += 1) {
      metadata = upsertPipeline(metadata, `pipeline-${index}`, "base64", [{ transformerId: "base64-decode" }], index);
    }

    expect(metadata.pipelines).toHaveLength(WORKFLOW_STORAGE_LIMITS.maxPipelines);
    expect(nextPipelineId(metadata)).toBeNull();
    expect(metadata.pipelines.some((pipeline) => pipeline.id === "pipeline-1")).toBe(true);
  });

  it("round-trips metadata across a restart using the browser preview store", async () => {
    const storage = new MemoryStorage();
    const persistence = createWorkflowPersistence({ storage, toolIds, transformerIds });
    let metadata = emptyMetadata();
    metadata = upsertPipeline(metadata, "pipeline-1", "base64", [{ transformerId: "base64-decode" }], 42);

    await persistence.load();
    await persistence.save(metadata);
    const restarted = await createWorkflowPersistence({ storage, toolIds, transformerIds }).load();

    expect(restarted).toEqual(metadata);
    expect(storage.body("workflows")).toContain("inputType");
    expect(storage.body("workflows")).not.toContain("output");
  });

  it("preserves malformed browser metadata and blocks an automatic replacement", async () => {
    const storage = new MemoryStorage();
    const malformed = '{"schemaVersion":1,"input":"credential-value"}';
    storage.seed("workflows", malformed);
    const persistence = createWorkflowPersistence({ storage, toolIds, transformerIds });

    await expect(persistence.load()).rejects.toThrow(WORKFLOW_STORAGE_ERROR);
    await expect(persistence.save(emptyMetadata())).rejects.toThrow(WORKFLOW_STORAGE_ERROR);
    expect(storage.body("workflows")).toBe(malformed);
  });
});
