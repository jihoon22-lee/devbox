import { describe, expect, it } from "vitest";
import type { ProjectProfile } from "../api";
import {
  draftFromProfile,
  emptyProfileDraft,
  ENVIRONMENT_REVISION_BYTES,
  MAX_ENVIRONMENT_NAME_BYTES,
  MAX_ENVIRONMENT_SOURCE_BYTES,
  MAX_ENVIRONMENT_VARIABLES,
  MAX_EXPECTED_PORTS,
  MAX_PROFILE_PATH_BYTES,
  MAX_SERVICES,
  newServiceDraftRow,
  parseExpectedPorts,
  validateProfileDraft,
} from "./profileEditor";

const profile: ProjectProfile = {
  id: "p-1",
  name: "devbox",
  windowsPath: "E:\\projects\\devbox",
  wsl: { distro: "Ubuntu", path: "/mnt/e/projects/devbox" },
  gitRoot: "E:\\projects\\devbox",
  expectedPorts: [3000, 5173],
  runManagerServiceIds: ["devbox-dev"],
  environment: null,
};

describe("profile editor draft", () => {
  it("keeps a profile's service rows addressable and preserves input text", () => {
    const draft = draftFromProfile(profile);
    expect(draft.expectedPortsText).toBe("3000, 5173");
    expect(draft.serviceRows).toHaveLength(1);
    expect(draft.serviceRows[0]?.value).toBe("devbox-dev");
    expect(newServiceDraftRow().key).not.toBe(draft.serviceRows[0]?.key);
  });

  it("parses only complete, unique TCP port values", () => {
    expect(parseExpectedPorts("3000, 5173")).toEqual({ ports: [3000, 5173] });
    expect(parseExpectedPorts("3000,,5173").error).toBeTruthy();
    expect(parseExpectedPorts("65536").error).toBeTruthy();
    expect(parseExpectedPorts("3000, 3000").error).toBeTruthy();
  });

  it("blocks invalid drafts without losing the raw invalid value", () => {
    const draft = { ...emptyProfileDraft(), name: "devbox", windowsPath: "E:\\projects\\devbox", expectedPortsText: "5173, nope" };
    const validation = validateProfileDraft(draft);
    expect(validation.profile).toBeNull();
    expect(validation.errors.expectedPorts).toBeTruthy();
    expect(draft.expectedPortsText).toBe("5173, nope");
  });

  it("creates the storage DTO only after service and port validation", () => {
    const draft = {
      ...draftFromProfile(profile),
      expectedPortsText: " 3000, 5173 ",
      serviceRows: [
        { key: "first", value: " devbox-dev " },
        { key: "second", value: "worker" },
      ],
    };
    const validation = validateProfileDraft(draft);
    expect(validation.errors.serviceRows).toEqual({});
    expect(validation.profile).toEqual({
      ...profile,
      expectedPorts: [3000, 5173],
      runManagerServiceIds: ["devbox-dev", "worker"],
    });
  });

  it("rejects an empty CRUD row and duplicate service IDs", () => {
    const draft = {
      ...draftFromProfile(profile),
      serviceRows: [
        { key: "first", value: "devbox-dev" },
        { key: "second", value: "devbox-dev" },
        { key: "empty", value: "" },
      ],
    };
    const validation = validateProfileDraft(draft);
    expect(validation.profile).toBeNull();
    expect(validation.errors.serviceRows.second).toContain("두 번");
    expect(validation.errors.serviceRows.empty).toContain("입력");
  });

  it("enforces bounded ports, paths, and service rows before building a DTO", () => {
    const ports = Array.from({ length: MAX_EXPECTED_PORTS + 1 }, (_, index) => String(index + 1)).join(",");
    expect(parseExpectedPorts(ports).error).toContain("최대 128개");
    expect(parseExpectedPorts("x".repeat(8193)).error).toContain("너무 깁니다");

    const longPath = `C:\\${"a".repeat(MAX_PROFILE_PATH_BYTES)}`;
    const tooManyServices = Array.from({ length: MAX_SERVICES + 1 }, (_, index) => ({
      key: `service-${index}`,
      value: `service-${index}`,
    }));
    const validation = validateProfileDraft({
      ...emptyProfileDraft(),
      name: "devbox",
      windowsPath: longPath,
      serviceRows: tooManyServices,
    });

    expect(validation.profile).toBeNull();
    expect(validation.errors.projectPath).toContain("너무 깁니다");
    expect(validation.errors.services).toContain("최대 128개");
  });

  it("rejects invalid editor IDs without echoing their contents", () => {
    const secretId = " secret-service";
    const validation = validateProfileDraft({
      ...draftFromProfile(profile),
      id: secretId,
    });
    expect(validation.profile).toBeNull();
    expect(validation.errors.id).toBe("프로필 ID가 올바르지 않습니다.");
    expect(validation.errors.id).not.toContain(secretId);
  });

  it("mirrors the native WSL distro argv boundary", () => {
    const validation = validateProfileDraft({
      ...draftFromProfile(profile),
      wslDistro: "Ubuntu;unexpected",
    });
    expect(validation.profile).toBeNull();
    expect(validation.errors.wsl).toBe("WSL distro 이름에 허용되지 않는 문자가 있습니다.");
  });

  it("round-trips environment metadata without adding values to the storage DTO", () => {
    const environment = {
      enabled: true,
      source: ".env.local",
      revision: "a".repeat(ENVIRONMENT_REVISION_BYTES),
      variables: [
        {
          name: "API_TOKEN",
          source: ".env.local",
          conflict: "none" as const,
          secretReference: { kind: "secret-ref/v1" as const, name: "API_TOKEN" },
        },
        {
          name: "NODE_ENV",
          source: ".env.local",
          conflict: "none" as const,
          secretReference: null,
        },
      ],
    };
    const draft = draftFromProfile({ ...profile, environment });
    expect(draft.environmentRevision).toBe(environment.revision);
    expect(draft.environmentPreview).toBeNull();
    const validation = validateProfileDraft(draft);
    expect(validation.errors.environment).toBeUndefined();
    expect(validation.profile?.environment).toEqual(environment);
    expect(JSON.stringify(validation.profile)).not.toContain("secret-value");
  });

  it("requires a native revision and rejects unsafe or over-bounded environment metadata", () => {
    const invalidSource = validateProfileDraft({
      ...emptyProfileDraft(),
      name: "devbox",
      windowsPath: "E:\\projects\\devbox",
      environmentSource: "../.env",
    });
    expect(invalidSource.profile).toBeNull();
    expect(invalidSource.errors.environment).toContain(".env");

    const withoutPreview = validateProfileDraft({
      ...emptyProfileDraft(),
      name: "devbox",
      windowsPath: "E:\\projects\\devbox",
      environmentSource: ".env",
    });
    expect(withoutPreview.profile).toBeNull();
    expect(withoutPreview.errors.environment).toContain("확인");

    const bounded = validateProfileDraft({
      ...emptyProfileDraft(),
      name: "devbox",
      windowsPath: "E:\\projects\\devbox",
      environmentSource: ".env",
      environmentRevision: "a".repeat(64),
      environmentVariables: Array.from({ length: MAX_ENVIRONMENT_VARIABLES + 1 }, (_, index) => ({
        name: `KEY_${index}`,
        source: ".env",
        conflict: "none" as const,
        secretReference: null,
      })),
    });
    expect(bounded.profile).toBeNull();
    expect(bounded.errors.environment).toContain("128개");
    expect(MAX_ENVIRONMENT_SOURCE_BYTES).toBe(256);
    expect(MAX_ENVIRONMENT_NAME_BYTES).toBe(128);
  });

  it("blocks enabled duplicate/reserved metadata but permits an explicitly disabled selection", () => {
    const conflict = {
      name: "PATH",
      source: ".env",
      conflict: "reserved" as const,
      secretReference: null,
    };
    const enabled = validateProfileDraft({
      ...emptyProfileDraft(),
      name: "devbox",
      windowsPath: "E:\\projects\\devbox",
      environmentEnabled: true,
      environmentSource: ".env",
      environmentRevision: "a".repeat(64),
      environmentVariables: [conflict],
    });
    expect(enabled.profile).toBeNull();
    expect(enabled.errors.environment).toContain("충돌");

    const disabled = validateProfileDraft({
      ...emptyProfileDraft(),
      name: "devbox",
      windowsPath: "E:\\projects\\devbox",
      environmentSource: ".env",
      environmentRevision: "a".repeat(64),
      environmentVariables: [conflict],
    });
    expect(disabled.profile).not.toBeNull();
    expect(disabled.profile?.environment?.enabled).toBe(false);
  });

  it("round-trips parser metadata when both duplicate occurrences are marked", () => {
    const validation = validateProfileDraft({
      ...emptyProfileDraft(),
      name: "devbox",
      windowsPath: "E:\\projects\\devbox",
      environmentSource: ".env",
      environmentRevision: "a".repeat(64),
      environmentVariables: [
        { name: "TOKEN", source: ".env", conflict: "duplicate", secretReference: { kind: "secret-ref/v1", name: "TOKEN" } },
        { name: "token", source: ".env", conflict: "duplicate", secretReference: { kind: "secret-ref/v1", name: "token" } },
      ],
    });
    expect(validation.errors.environment).toBeUndefined();
    expect(validation.profile?.environment?.variables).toHaveLength(2);
  });

  it("fails closed when a malformed preview omits the nullable secret reference", () => {
    const malformed = {
      ...emptyProfileDraft(),
      name: "devbox",
      windowsPath: "E:\\projects\\devbox",
      environmentSource: ".env",
      environmentRevision: "a".repeat(64),
      environmentVariables: [{
        name: "API_TOKEN",
        source: ".env",
        conflict: "none" as const,
        secretReference: undefined,
      }],
    } as never;
    expect(() => validateProfileDraft(malformed)).not.toThrow();
    expect(validateProfileDraft(malformed).errors.environment).toContain("reference");
  });
});
