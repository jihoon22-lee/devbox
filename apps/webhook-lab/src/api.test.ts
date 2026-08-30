import { describe, expect, it } from "vitest";
import { exportRunServiceDefinition } from "./api";

describe("Webhook Lab browser API mocks", () => {
  it("returns an import-valid disabled Run Manager definition", async () => {
    const definition = await exportRunServiceDefinition();
    expect(definition).toMatchObject({
      schemaVersion: 1,
      jobs: [],
    });
    expect(definition.services).toHaveLength(1);

    const service = definition.services[0];
    expect(service).toMatchObject({
      id: "00000000-0000-4000-8000-000000000001",
      kind: "service",
      command: "exit /b 1",
      enabled: false,
      autoStart: false,
      restartPolicy: "never",
      envConfigured: false,
      targetKind: "windows",
    });
    expect(service.id).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/u);
    expect(service.createdAt).toBeGreaterThan(0);
    expect(service.updatedAt).toBeGreaterThan(0);
  });
});
