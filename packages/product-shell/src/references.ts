import { matchesProvenance, type Provenance } from "./operation";

function shape(value: unknown, keys: string): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value)
    && Object.keys(value).sort().join(",") === keys;
}

// Metadata readers only: the shell cannot launch a target or resolve a secret.
export function isApiArtifactReference(value: unknown, expected: Provenance): boolean {
  if (expected.product !== "api-studio" || expected.component !== "api-studio.api"
    || !shape(value, "link,provenance,recipient") || !matchesProvenance(value.provenance, expected)
    || value.recipient !== "developer-toolbox" || !shape(value.link, "from,target")
    || value.link.from !== "api-playground" || !shape(value.link.target, "handoffKind,id,kind")) return false;
  return value.link.target.kind === "handoff" && value.link.target.handoffKind === "toolbox-text/v1"
    && typeof value.link.target.id === "string" && /^[a-f0-9]{32}$/.test(value.link.target.id);
}

export type SecretOwner = "api-environment" | "runtime-environment" | "project-environment";
const owners: Record<SecretOwner, [string, string]> = {
  "api-environment": ["api-studio", "api-studio.api"],
  "runtime-environment": ["workspace", "workspace.runtime"],
  "project-environment": ["workspace", "workspace.project"],
};
export function isOwnedSecretReference(value: unknown, expected: Provenance, owner: SecretOwner): boolean {
  const [product, component] = owners[owner];
  return expected.product === product && expected.component === component
    && shape(value, "owner,provenance,reference") && value.owner === owner
    && matchesProvenance(value.provenance, expected) && shape(value.reference, "kind,name")
    && value.reference.kind === "secret-ref/v1" && typeof value.reference.name === "string"
    && /^[A-Za-z_][A-Za-z0-9_]{0,127}$/.test(value.reference.name);
}
