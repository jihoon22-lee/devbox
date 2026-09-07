import { describe, expect, it, vi } from "vitest";
import { readResponseBytes } from "./api";

function nullBodyResponse(
  contentLength: string | null,
  bytes: Uint8Array,
): Response {
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
