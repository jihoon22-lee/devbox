import { missingVariables } from "./runner";
import { expect, it } from "vitest";
import { emptyRequest } from "./importers";
import { sanitizeRequestForPersistence, toRequestTemplate } from "./persistence";
import { addEntry, emptyStore, parseStore } from "./collections";
import { serializeCollectionExport, parseCollectionExport } from "./transfer";
import { serializeFileCollection, parseFileCollection } from "./fileCollection";
const config = {
  grantType: "authorizationCode" as const,
  authorizationUrl: "https://auth.test/authorize",
  tokenUrl: "https://auth.test/token",
  clientId: "client",
  clientSecret: "{{CLIENT_SECRET}}",
  scopes: "read write",
};
const request = () => ({
  ...emptyRequest(),
  url: "https://api.test/me",
  auth: { kind: "oauth2", username: "", password: "", token: "", api_key: "", api_value: "", oauth2: { ...config } },
});
it("redacts literal client secrets and preserves references", () => {
  const input = request();
  input.auth.oauth2.clientSecret = "literal-private-secret";
  const safe = sanitizeRequestForPersistence(input);
  expect(safe.auth?.oauth2?.clientSecret).toBe("[REDACTED]");
  expect(safe.requiresSecretReview).toBe(true);
  expect(sanitizeRequestForPersistence(request()).auth?.oauth2?.clientSecret).toBe("{{CLIENT_SECRET}}");
});
it("round trips OAuth configuration through native documents, JSON and file collections", () => {
  const store = addEntry(emptyStore(), { name: "OAuth", folder: "", request: request() }, 1, () => "oauth");
  const reopened = parseStore(JSON.stringify(store))!;
  expect(toRequestTemplate(reopened.collections[0].request).auth?.oauth2).toEqual(config);
  const raw = serializeCollectionExport(store);
  expect(parseCollectionExport(raw)?.collections[0].request.auth?.oauth2).toEqual(config);
  expect(parseFileCollection(serializeFileCollection("OAuth", store)).requests[0].request.auth?.oauth2).toEqual(config);
});
it("keeps old requests unchanged and drops unregistered OAuth fields", () => {
  expect(sanitizeRequestForPersistence(emptyRequest()).auth).toEqual(emptyRequest().auth);
  const input = request();
  Object.assign(input.auth.oauth2, { rawBackup: "private-backup" });
  const safe = sanitizeRequestForPersistence(input);
  expect(JSON.stringify(safe)).not.toContain("private-backup");
  const editable = toRequestTemplate(safe);
  editable.auth!.oauth2!.clientId = "changed";
  expect(safe.auth?.oauth2?.clientId).toBe("client");
});

it("ignores an inactive authorization URL when checking client-credentials variables", () => {
  const req = {
    ...request(),
    auth: {
      ...request().auth,
      oauth2: { ...config, grantType: "clientCredentials" as const, authorizationUrl: "{{UNUSED_AUTH}}" },
    },
  };
  expect(missingVariables(req, new Set(["CLIENT_SECRET"]))).toEqual([]);
  expect(
    missingVariables(
      { ...req, auth: { ...req.auth, oauth2: { ...req.auth.oauth2, grantType: "authorizationCode" } } },
      new Set(["CLIENT_SECRET"]),
    ),
  ).toEqual(["UNUSED_AUTH"]);
});
