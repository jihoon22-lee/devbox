import { beforeEach, describe, expect, it, vi } from "vitest";

const mermaidMock = vi.hoisted(() => ({
  initialize: vi.fn(),
  render: vi.fn<(id: string, source: string) => Promise<{ svg: string }>>(),
}));

vi.mock("mermaid", () => ({ default: mermaidMock }));

let getMermaidRenderer: typeof import("./index").getMermaidRenderer;

beforeEach(async () => {
  vi.resetModules();
  mermaidMock.initialize.mockReset();
  mermaidMock.render.mockReset().mockResolvedValue({ svg: "<svg />" });
  ({ getMermaidRenderer } = await import("./index"));
});

describe("getMermaidRenderer", () => {
  it("does not initialize Mermaid until the getter is requested", async () => {
    expect(mermaidMock.initialize).not.toHaveBeenCalled();

    await getMermaidRenderer();

    expect(mermaidMock.initialize).toHaveBeenCalledTimes(1);
    expect(mermaidMock.initialize).toHaveBeenCalledWith({
      startOnLoad: false,
      theme: "dark",
      securityLevel: "strict",
    });
  });

  it("shares one in-flight initialization promise across concurrent callers", async () => {
    const first = getMermaidRenderer();
    const second = getMermaidRenderer();

    expect(first).toBe(second);
    await Promise.all([first, second]);
    expect(mermaidMock.initialize).toHaveBeenCalledTimes(1);

    await getMermaidRenderer();
    expect(mermaidMock.initialize).toHaveBeenCalledTimes(1);
  });

  it("clears a failed initialization so a later call can retry", async () => {
    const failure = new Error("Mermaid unavailable");
    mermaidMock.initialize.mockImplementationOnce(() => {
      throw failure;
    });

    await expect(getMermaidRenderer()).rejects.toBe(failure);
    await expect(getMermaidRenderer()).resolves.toBe(mermaidMock);
    expect(mermaidMock.initialize).toHaveBeenCalledTimes(2);
  });
});
