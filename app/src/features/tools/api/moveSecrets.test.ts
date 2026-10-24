import { expect, it } from "vitest";
import { emptyDraft, row } from "./model/draft";
import { MoveSecretsError, moveToSecrets } from "./moveSecrets";

it("a_failure_midway_reports_the_secrets_already_created", async () => {
  const created = new Map<string, string>();
  const backend = {
    secretNames: async () => [],
    setSecret: async (name: string, value: string) => {
      if (created.size === 1) throw new Error("locked");
      created.set(name, value);
    },
  };
  const draft = { ...emptyDraft(), headers: [row("Authorization", "Bearer one"), row("X-Api-Key", "two")] };
  const error = await moveToSecrets(backend, draft, "A", ["request.headers.0.value", "request.headers.1.value"]).catch(
    (e: unknown) => e,
  );
  expect(error).toBeInstanceOf(MoveSecretsError);
  expect((error as MoveSecretsError).created).toEqual(["A_AUTHORIZATION"]);
});
