import { afterEach, describe, expect, it, vi } from "vitest";
import { readResponseBytes, sendRequest } from "./api";

function nullBodyResponse(contentLength: string | null, bytes: Uint8Array): Response {
  const headers = new Headers();
  if (contentLength !== null) headers.set("content-length", contentLength);
  return {
    body: null,
    headers,
    arrayBuffer: vi.fn(async () => bytes.slice().buffer),
  } as unknown as Response;
}

describe("browser response byte reader", () => {
  it("treats a null-body 204/HEAD response as empty without reading arrayBuffer", async () => {
    const response = nullBodyResponse(null, new Uint8Array([1, 2, 3]));
    const arrayBuffer = response.arrayBuffer as ReturnType<typeof vi.fn>;

    await expect(readResponseBytes(response, 16)).resolves.toEqual(new Uint8Array());
    expect(arrayBuffer).not.toHaveBeenCalled();
  });

  it("does not trust a contradictory Content-Length when the fetch body is null", async () => {
    for (const declared of ["-1", "1.5", "not-a-number", "17"]) {
      const response = nullBodyResponse(declared, new Uint8Array([1]));
      const arrayBuffer = response.arrayBuffer as ReturnType<typeof vi.fn>;

      await expect(readResponseBytes(response, 16)).resolves.toEqual(new Uint8Array());
      expect(arrayBuffer).not.toHaveBeenCalled();
    }
  });

  it("keeps the streaming path bounded for known response bytes", async () => {
    const response = new Response(new Uint8Array([1, 2, 3]), {
      headers: { "content-length": "3" },
    });
    await expect(readResponseBytes(response, 3)).resolves.toEqual(new Uint8Array([1, 2, 3]));
  });

  it("checks the actual streamed body against the bound", async () => {
    const response = new Response(new Uint8Array([1, 2, 3, 4]), {
      headers: { "content-length": "3" },
    });
    await expect(readResponseBytes(response, 3)).rejects.toThrow("허용된 크기를 초과했습니다");
  });
});

import { emptyRequest } from "./lib/importers";
afterEach(() => vi.unstubAllGlobals());
it("preserves encoded query and form values and excludes a hidden None body", async () => {
  const fetchMock = vi.fn(async () => new Response("ok"));
  vi.stubGlobal("fetch", fetchMock);
  const pairs = [["q", "a&admin=true"], ["q", "a#b"], ["plus", "a+b"], ["한글", " 한 글 "], ["empty", ""]];
  const request = { ...emptyRequest(), method: "POST", url: "https://example.test/?existing=ok#fragment", params: pairs.map(([key, value]) => ({key, value})), body_kind: "form", body: pairs.map(([key, value]) => `${key}=${value}`).join("\n") };
  await sendRequest(request, []);
  const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
  expect([...new URL(url).searchParams]).toEqual([["existing", "ok"], ...pairs]);
  expect(new URL(url).hash).toBe("#fragment");
  expect([...new URLSearchParams(init.body as string)]).toEqual(pairs);
  await sendRequest({...request, body_kind: "none", body: "{{hidden}}"}, []);
  expect((fetchMock.mock.calls[1] as unknown as [string, RequestInit])[1].body).toBeUndefined();
});
