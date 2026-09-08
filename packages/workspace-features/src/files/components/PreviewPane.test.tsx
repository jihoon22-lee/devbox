import { cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MermaidRenderer } from "@devbox/mermaid-renderer";
import { getMermaidRenderer } from "@devbox/mermaid-renderer";
import PreviewPane from "./PreviewPane";
import type { PreviewResponse } from "../types";

vi.mock("@devbox/mermaid-renderer", () => ({
  getMermaidRenderer: vi.fn(),
}));

const getMermaidRendererMock = vi.mocked(getMermaidRenderer);
const renderMock = vi.fn<MermaidRenderer["render"]>();
const renderer: MermaidRenderer = {
  initialize: vi.fn(),
  render: renderMock,
};

function markdownResponse(html: string, mermaid: string[] = []): PreviewResponse {
  return { kind: "markdown", html, mermaid };
}

function bodyOf(rendered: ReturnType<typeof render>): HTMLElement {
  const body = rendered.container.querySelector(".preview-body");
  if (!(body instanceof HTMLElement)) throw new Error("Preview body was not rendered");
  return body;
}

beforeEach(() => {
  getMermaidRendererMock.mockReset().mockResolvedValue(renderer);
  renderMock.mockReset().mockResolvedValue({ svg: '<svg data-rendered="good"></svg>' });
});

afterEach(() => cleanup());

describe("PreviewPane Mermaid loading", () => {
  it("does not request Mermaid for ordinary Markdown", () => {
    const rendered = render(
      <PreviewPane docPath="README.md" response={markdownResponse("<p>plain text</p>")} error={null} />,
    );

    expect(bodyOf(rendered).textContent).toBe("plain text");
    expect(getMermaidRendererMock).not.toHaveBeenCalled();
  });

  it("requests the shared renderer only for Markdown Mermaid blocks", async () => {
    const rendered = render(
      <PreviewPane
        docPath="README.md"
        response={markdownResponse('<div class="mermaid-block" data-idx="0"></div>', ["graph TD; A-->B;"])}
        error={null}
      />,
    );

    await waitFor(() => expect(getMermaidRendererMock).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(renderMock).toHaveBeenCalledWith(expect.any(String), "graph TD; A-->B;"));
    expect(bodyOf(rendered).innerHTML).toContain('data-rendered="good"');
  });

  it("keeps the last-good SVG and shows an error badge after a failed render", async () => {
    const first = markdownResponse('<div class="mermaid-block" data-idx="0"></div>', ["graph TD; A-->B;"]);
    const second = markdownResponse('<div class="mermaid-block" data-idx="0"></div>', ["invalid"]);
    const rendered = render(<PreviewPane docPath="README.md" response={first} error={null} />);

    await waitFor(() => expect(bodyOf(rendered).innerHTML).toContain('data-rendered="good"'));
    renderMock.mockRejectedValueOnce(new Error("syntax error"));
    rendered.rerender(<PreviewPane docPath="README.md" response={second} error={null} />);

    await waitFor(() => expect(bodyOf(rendered).querySelector(".mermaid-error-badge")).toBeTruthy());
    expect(bodyOf(rendered).innerHTML).toContain('data-rendered="good"');
  });

  it("does not apply a stale render after the preview response changes", async () => {
    let resolveRender!: (result: { svg: string }) => void;
    renderMock.mockImplementationOnce(() => new Promise((resolve) => {
      resolveRender = resolve;
    }));
    const rendered = render(
      <PreviewPane
        docPath="README.md"
        response={markdownResponse('<div class="mermaid-block" data-idx="0"></div>', ["old"])}
        error={null}
      />,
    );
    await waitFor(() => expect(renderMock).toHaveBeenCalledTimes(1));

    rendered.rerender(<PreviewPane docPath="README.md" response={markdownResponse("<p>new</p>")} error={null} />);
    resolveRender({ svg: '<svg data-rendered="stale"></svg>' });

    await waitFor(() => expect(bodyOf(rendered).textContent).toBe("new"));
    expect(bodyOf(rendered).innerHTML).not.toContain("stale");
  });
});
