import { expect, test } from "vitest";
import { ensureBottomSession } from "./bottomSession";

test("StrictMode consumers share one creation per client/project and failures allow explicit retry", async () => {
  const client = {};
  let calls = 0;
  let finish!: (id: string) => void;
  const create = () => {
    calls++;
    return new Promise<string>((resolve) => {
      finish = resolve;
    });
  };
  const first = ensureBottomSession(client, "p", create);
  const second = ensureBottomSession(client, "p", create);
  expect(calls).toBe(1);
  finish("created");
  expect(await first).toBe("created");
  expect(await second).toBe("created");
  await expect(ensureBottomSession(client, "other", () => Promise.reject(new Error("offline")))).rejects.toThrow(
    "offline",
  );
  await expect(ensureBottomSession(client, "other", () => Promise.resolve("retried"))).resolves.toBe("retried");
});
