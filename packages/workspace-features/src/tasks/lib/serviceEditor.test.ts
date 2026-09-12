import { describe, expect, it } from "vitest";
import {
  EMPTY_SERVICE_DRAFT,
  isLocalTcpAddress,
  toServiceInput,
  validateServiceDraft,
} from "./serviceEditor";

describe("service editor validation", () => {
  const valid = { ...EMPTY_SERVICE_DRAFT, name: "web", command: "npm start" };

  it("uses the fixed ten-second grace model and keeps environment protected", () => {
    expect(toServiceInput(valid)).toMatchObject({
      restartPolicy: "never",
      autoStart: false,
      healthTcpAddress: null,
      healthTcpPort: null,
      environment: { action: "keep" },
    });
  });

  it("accepts loopback endpoints and rejects remote or shell-like addresses", () => {
    expect(isLocalTcpAddress("localhost")).toBe(true);
    expect(isLocalTcpAddress("127.0.0.1")).toBe(true);
    expect(isLocalTcpAddress("::1")).toBe(true);
    expect(isLocalTcpAddress("192.168.1.10")).toBe(false);
    expect(isLocalTcpAddress("127.0.0.1;curl evil")).toBe(false);
  });

  it("validates an optional TCP address and port as a pair", () => {
    expect(validateServiceDraft({ ...valid, healthTcpEnabled: true }).healthTcpAddress).toBeUndefined();
    expect(validateServiceDraft({ ...valid, healthTcpEnabled: true }).healthTcpPort).toContain("포트");
    expect(
      validateServiceDraft({
        ...valid,
        healthTcpEnabled: true,
        healthTcpAddress: "127.0.0.1",
        healthTcpPort: "8080",
      }),
    ).toEqual({});
    expect(validateServiceDraft({
      ...valid,
      environmentAction: "replace",
      environment: [{ id: "env", key: "BAD-NAME", value: "secret", persisted: false }],
    }).env).toContain("환경변수");
  });
});
