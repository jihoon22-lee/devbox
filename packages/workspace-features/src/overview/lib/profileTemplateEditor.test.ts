import { describe, expect, it } from "vitest";
import type { ProfileTemplate } from "../api";
import {
  emptyProfileTemplateDraft,
  profileDraftFromTemplate,
  templateDraftFromTemplate,
  validateProfileTemplateDraft,
} from "./profileTemplateEditor";

const template: ProfileTemplate = {
  id: "template-node",
  name: "Node service",
  windowsPath: null,
  wsl: { distro: "Ubuntu", path: "/mnt/e/projects/node" },
  gitRoot: null,
  expectedPorts: [3000, 5173],
  runManagerServiceIds: ["node-dev"],
};

describe("profile template editor", () => {
  it("round-trips bounded defaults without an environment field", () => {
    const draft = templateDraftFromTemplate(template);
    const result = validateProfileTemplateDraft(draft);
    expect(result.errors).toEqual({});
    expect(result.template).toEqual(template);
    expect(JSON.stringify(result.template)).not.toContain("secret");
  });

  it("applies template defaults to a wizard draft while leaving name editable", () => {
    const draft = profileDraftFromTemplate(template);
    expect(draft.name).toBe("");
    expect(draft.wslDistro).toBe("Ubuntu");
    expect(draft.expectedPortsText).toBe("3000, 5173");
    expect(draft.serviceRows.map((row) => row.value)).toEqual(["node-dev"]);
    expect(draft.environmentVariables).toEqual([]);
  });

  it("allows a path-less reusable template but rejects unsafe service/port input", () => {
    const empty = emptyProfileTemplateDraft();
    expect(validateProfileTemplateDraft({ ...empty, name: "Generic" }).template).not.toBeNull();
    const invalid = validateProfileTemplateDraft({
      ...empty,
      name: "Generic",
      expectedPortsText: "3000,,5173",
      serviceIdsText: "node-dev, node-dev",
    });
    expect(invalid.template).toBeNull();
    expect(invalid.errors.expectedPorts).toBeTruthy();
    expect(invalid.errors.services).toContain("두 번");
  });

  it("rejects relative, traversal, device, and unsafe Windows defaults", () => {
    for (const windowsPath of [
      "relative/project",
      "C:/work/../escape",
      "C:/work/NUL.txt",
      "\\\\?\\C:\\work\\devbox",
    ]) {
      const result = validateProfileTemplateDraft({
        ...emptyProfileTemplateDraft(),
        name: "Unsafe",
        windowsPath,
      });
      expect(result.template).toBeNull();
      expect(result.errors.projectPath).toBeTruthy();
    }
    const safe = validateProfileTemplateDraft({
      ...emptyProfileTemplateDraft(),
      name: "Windows",
      windowsPath: "C:/work/devbox",
    });
    expect(safe.template?.windowsPath).toBe("C:/work/devbox");
  });
});
