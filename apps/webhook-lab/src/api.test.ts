import { describe, expect, it, vi } from "vitest";
import {
  exportRunServiceDefinition,
  sendFixtureToLogLens,
  sendHistoryToLogLens,
} from "./api";

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

  it("rejects Log Lens handoff in a browser without clipboard fallback", async () => {
    const writeText = vi.fn<(value: string) => Promise<void>>();
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });

    await expect(sendHistoryToLogLens(1)).rejects.toThrow(
      "앱 간 handoff는 데스크톱 앱에서만 사용할 수 있습니다. 클립보드로 자동 전환하지 않습니다",
    );
    await expect(sendFixtureToLogLens("fixture-1")).rejects.toThrow(
      "앱 간 handoff는 데스크톱 앱에서만 사용할 수 있습니다. 클립보드로 자동 전환하지 않습니다",
    );
    expect(writeText).not.toHaveBeenCalled();
  });
});
