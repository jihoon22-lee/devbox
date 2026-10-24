import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { Preview } from "./Preview";

const rpc = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock("../../rpc/RpcProvider", () => ({ useRpcSession: () => ({ client: rpc, state: "ready" }) }));
vi.mock("mermaid", () => ({
  default: {
    initialize: vi.fn(),
    render: vi.fn(async () => ({ svg: '<svg xmlns="http://www.w3.org/2000/svg"><text>safe graph</text></svg>' })),
  },
}));
const key = { root: "project:p", path: "docs/readme.md" };
let blobs: Blob[], revoked: string[];
beforeEach(() => {
  blobs = [];
  revoked = [];
  vi.stubGlobal("URL", URL);
  URL.createObjectURL = vi.fn((blob) => {
    blobs.push(blob as Blob);
    return `blob:owned-${blobs.length}`;
  });
  URL.revokeObjectURL = vi.fn((uri) => {
    revoked.push(uri);
  });
});
afterEach(() => {
  cleanup();
  rpc.call.mockReset();
  vi.unstubAllGlobals();
});
test("dirty markdown is sent to project preview; old responses cannot replace the current buffer preview", async () => {
  let old: (result: unknown) => void = () => {};
  rpc.call
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          old = resolve;
        }),
    )
    .mockResolvedValue({ kind: "markdown", rendered: { html: "<strong>new buffer</strong>", mermaid: [] } });
  const shown = render(<Preview docKey={key} text="old buffer" />);
  await waitFor(() => expect(rpc.call).toHaveBeenCalledWith("files.preview", { key, text: "old buffer" }));
  shown.rerender(<Preview docKey={key} text="new buffer" />);
  await screen.findByText("new buffer");
  await act(async () => old({ kind: "markdown", rendered: { html: "old buffer", mermaid: [] } }));
  expect(screen.queryByText("old buffer")).not.toBeInTheDocument();
});
test("malicious SVG becomes only a sanitized blob image and old URLs are revoked", async () => {
  const bad =
    '<svg xmlns="http://www.w3.org/2000/svg" onload="BAD()"><script>BAD()</script><image href="https://fixture.invalid/leak"/><foreignObject><iframe src="https://fixture.invalid/frame"/></foreignObject><rect width="10" height="10" onclick="BAD()"/></svg>';
  rpc.call.mockResolvedValue({ kind: "svg", text: bad });
  const shown = render(<Preview docKey={{ ...key, path: "a.svg" }} text={bad} />);
  const img = await screen.findByRole("img", { name: "SVG 미리보기" });
  expect(img).toHaveAttribute("src", "blob:owned-1");
  const text = await new Promise<string>((resolve) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result as string);
    reader.readAsText(blobs[0]!);
  });
  expect(text).toContain("<rect");
  expect(text).not.toMatch(/script|onload|onclick|foreignObject|iframe|fixture.invalid/);
  expect(document.querySelector("svg,script,iframe")).toBeNull();
  shown.rerender(<Preview docKey={{ ...key, path: "a.svg" }} text={`${bad} `} />);
  await waitFor(() => expect(screen.getByRole("img", { name: "SVG 미리보기" })).toHaveAttribute("src", "blob:owned-2"));
  expect(revoked).toContain("blob:owned-1");
  shown.unmount();
  expect(revoked).toContain("blob:owned-2");
});
test("Markdown javascript, remote images and Mermaid external resources remain blocked in the shared renderer", async () => {
  rpc.call.mockResolvedValue({
    kind: "markdown",
    rendered: {
      html: '<script>BAD()</script><a href="javascript:BAD()">bad</a><img src="https://fixture.invalid/leak"><div class="mermaid-block" data-idx="0"></div>',
      mermaid: ['flowchart TD; A@{ img: "https://fixture.invalid/leak" }'],
    },
  });
  render(<Preview docKey={key} text="dirty" />);
  await screen.findByText("bad");
  expect(document.querySelector("script,img,a[href]")).toBeNull();
  await screen.findByText(/외부/);
});

test("changing the buffer hides a previously rendered preview before the next response", async () => {
  rpc.call
    .mockResolvedValueOnce({ kind: "markdown", rendered: { html: "previous content", mermaid: [] } })
    .mockImplementation(() => new Promise(() => {}));
  const shown = render(<Preview docKey={key} text="previous" />);
  await screen.findByText("previous content");
  shown.rerender(<Preview docKey={key} text="pending new content" />);
  expect(screen.queryByText("previous content")).toBeNull();
});
