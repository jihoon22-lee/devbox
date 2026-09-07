import { describe, expect, it } from "vitest";
import fixture from "../fixtures/references.json";
import { isApiArtifactReference, isOwnedSecretReference } from "./references";

describe("source-owned native references", () => {
  const expected = fixture.artifact.provenance;
  it("consumes the native fixture and rejects body/path/token and foreign targets", () => {
    expect(isApiArtifactReference(fixture.artifact, expected)).toBe(true);
    for (const field of ["body", "path", "claimToken"]) {
      expect(isApiArtifactReference({ ...fixture.artifact, [field]: "synthetic" }, expected)).toBe(false);
    }
    expect(isApiArtifactReference({ ...fixture.artifact, recipient: "knowledge-base" }, expected)).toBe(false);
    expect(isApiArtifactReference(fixture.artifact, { ...expected, revision: 2 })).toBe(false);
    expect(isApiArtifactReference({ ...fixture.artifact, link: { ...fixture.artifact.link, target: { kind: "path", path: "C:\\fixture" } } }, expected)).toBe(false);
  });
  it("accepts only the existing reference version and native-selected owner", () => {
    expect(isOwnedSecretReference(fixture.secret, expected, "api-environment")).toBe(true);
    expect(isOwnedSecretReference(fixture.secret, expected, "runtime-environment")).toBe(false);
    for (const reference of [{ kind: "secret-ref/v2", name: "API_TOKEN" }, { kind: "secret-ref/v1", name: "../value" }, { ...fixture.secret.reference, plaintext: "synthetic" }]) {
      expect(isOwnedSecretReference({ ...fixture.secret, reference }, expected, "api-environment")).toBe(false);
    }
  });
});
