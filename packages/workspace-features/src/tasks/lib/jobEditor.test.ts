import { describe, expect, it } from "vitest";
import {
  CRON_PRESETS,
  EMPTY_JOB_DRAFT,
  toJobInput,
  validateJobDraft,
} from "./jobEditor";

describe("cron builder presets", () => {
  it("exposes the five approved presets with optional-seconds expressions", () => {
    expect(CRON_PRESETS).toHaveLength(5);
    expect(CRON_PRESETS.map((preset) => preset.expression)).toEqual([
      "0 * * * * *",
      "0 0 * * * *",
      "0 0 9 * * *",
      "0 0 9 * * 1-5",
      "0 0 9 * * 1",
    ]);
  });

  it("keeps a five-field direct expression valid", () => {
    const errors = validateJobDraft({ ...EMPTY_JOB_DRAFT, name: "backup", command: "echo ok", cronExpr: "*/7 * * * *" });
    expect(errors.cronExpr).toBeUndefined();
  });
});

describe("job save validation", () => {
  const valid = { ...EMPTY_JOB_DRAFT, name: "backup", command: "python main.py" };

  it("reports invalid cron shape before persistence", () => {
    expect(validateJobDraft({ ...valid, cronExpr: "@reboot" }).cronExpr).toContain("매크로");
    expect(validateJobDraft({ ...valid, cronExpr: "0 0 *" }).cronExpr).toContain("5개 또는 6개");
  });

  it("requires a distro only for WSL and rejects one for Windows", () => {
    expect(validateJobDraft({ ...valid, targetKind: "wsl", targetDistro: "" }).targetDistro).toContain("배포판");
    expect(validateJobDraft({ ...valid, targetKind: "windows", targetDistro: "Ubuntu" }).targetDistro).toContain("Windows");
    expect(validateJobDraft({ ...valid, targetKind: "wsl", targetDistro: "Ubuntu" }).targetDistro).toBeUndefined();
  });

  it("sends a complete replacement map to the protected command boundary", () => {
    const input = toJobInput({
      ...valid,
      environment: [{ id: "new", key: "TOKEN", value: "secret", persisted: false }],
      environmentAction: "replace",
    });
    expect(input.environment).toEqual({ action: "replace", values: { TOKEN: "secret" } });
  });

  it("keeps masked values unless the user explicitly clears or replaces them", () => {
    const persisted = [{ id: "persisted", key: "", value: "", persisted: true }];
    expect(toJobInput({ ...valid, environment: persisted, environmentAction: "keep" }).environment).toEqual({
      action: "keep",
    });
    expect(toJobInput({ ...valid, environment: [], environmentAction: "clear" }).environment).toEqual({
      action: "clear",
    });
    expect(
      toJobInput({
        ...valid,
        environment: [{ id: "new", key: "TOKEN", value: "rotated", persisted: false }],
        environmentAction: "replace",
      }).environment,
    ).toEqual({ action: "replace", values: { TOKEN: "rotated" } });
  });

  it("validates new values without blocking on platform availability", () => {
    const errors = validateJobDraft({
      ...valid,
      environment: [{ id: "new", key: "TOKEN", value: "secret", persisted: false }],
      environmentAction: "replace",
    });
    expect(errors.env).toBeUndefined();
  });
});
