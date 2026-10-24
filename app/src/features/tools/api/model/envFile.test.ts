import { describe, expect, it } from "vitest";
import { envFilePath, environmentFromPreview, parseEnvFile, serializeEnvFile } from "./envFile";

describe("environment files", () => {
  it("secret_values_are_cleared_and_references_kept", () => {
    const { text, cleared } = serializeEnvFile({
      version: 1,
      name: "dev",
      variables: [
        { name: "baseUrl", value: "http://127.0.0.1:8080" },
        { name: "API_TOKEN", value: "raw-value" },
        { name: "token", value: "ghp_1234567890abcdefghij" },
        { name: "PASSWORD", value: "{{secret:DEV_PASSWORD}}" },
      ],
    });
    expect(text).not.toContain("raw-value");
    expect(text).not.toContain("ghp_");
    expect(cleared).toEqual(["API_TOKEN", "token"]);
    expect(parseEnvFile(text).variables).toEqual([
      { name: "baseUrl", value: "http://127.0.0.1:8080" },
      { name: "API_TOKEN", value: "", secret: true },
      { name: "token", value: "", secret: true },
      { name: "PASSWORD", value: "{{secret:DEV_PASSWORD}}" },
    ]);
  });

  it("invalid_files_and_names_are_rejected", () => {
    expect(() => parseEnvFile("{")).toThrow();
    expect(() => parseEnvFile(JSON.stringify({ version: 2, name: "x", variables: [] }))).toThrow();
    expect(() =>
      parseEnvFile(JSON.stringify({ version: 1, name: "x", variables: [{ name: "bad name", value: "" }] })),
    ).toThrow();
    const used = new Set<string>();
    expect(envFilePath("dev", used)).toBe("env/dev.json");
    expect(envFilePath("DEV", used)).toBe("env/DEV (2).json");
    expect(envFilePath("../x", new Set())).toBe("env/..-x.json");
  });

  it("preview_environments_keep_secret_marks", () => {
    const file = environmentFromPreview({
      name: "imported",
      variables: [
        { key: "host", value: "x.test", secret: false },
        { key: "API_KEY", value: "", secret: true },
      ],
    });
    expect(file.variables).toEqual([
      { name: "host", value: "x.test" },
      { name: "API_KEY", value: "", secret: true },
    ]);
  });
});
