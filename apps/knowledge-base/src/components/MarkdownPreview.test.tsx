import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MermaidRenderer } from "@devbox/mermaid-renderer";
import { getMermaidRenderer } from "@devbox/mermaid-renderer";
import MarkdownPreview from "./MarkdownPreview";

vi.mock("../api", () => ({
  openExternal: vi.fn(async () => undefined),
}));

vi.mock("@devbox/mermaid-renderer", () => ({
  getMermaidRenderer: vi.fn(),
}));

const getMermaidRendererMock = vi.mocked(getMermaidRenderer);
const renderMock = vi.fn<MermaidRenderer["render"]>();
const renderer: MermaidRenderer = {
  initialize: vi.fn(),
  render: renderMock,
};

beforeEach(() => {
  getMermaidRendererMock.mockReset().mockResolvedValue(renderer);
  renderMock.mockReset().mockResolvedValue({ svg: '<svg data-rendered="good"></svg>' });
});

afterEach(() => cleanup());

describe("MarkdownPreview wikilink navigation", () => {
  it("treats backend-resolved root hrefs as Knowledge-root-relative paths", () => {
    const onNavigate = vi.fn();
    const onNavigateWikilink = vi.fn();
    render(
      <MarkdownPreview
        baseRel="Projects/current.md"
        doc={{
          title: null,
          tags: [],
          html: '<a class="wikilink resolved" href="/Notes/Rust.md">Rust</a> <a href="../Reference/plain.md">Plain</a> <a href="/ordinary.md">Ordinary root-style</a>',
          mermaid: [],
        }}
        onNavigate={onNavigate}
        onNavigateWikilink={onNavigateWikilink}
      />,
    );

    fireEvent.click(screen.getByRole("link", { name: "Rust" }));
    expect(onNavigateWikilink).toHaveBeenCalledWith("Notes/Rust.md");
    expect(onNavigate).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("link", { name: "Plain" }));
    expect(onNavigate).toHaveBeenCalledWith("Reference/plain.md");

    fireEvent.click(screen.getByRole("link", { name: "Ordinary root-style" }));
    expect(onNavigate).toHaveBeenCalledWith("Projects/ordinary.md");
  });
});

describe("MarkdownPreview Mermaid loading", () => {
  it("does not request Mermaid for ordinary Markdown", () => {
    render(
      <MarkdownPreview
        baseRel="Notes/plain.md"
        doc={{ title: null, tags: [], html: "<p>plain text</p>", mermaid: [] }}
        onNavigate={() => undefined}
      />,
    );

    expect(getMermaidRendererMock).not.toHaveBeenCalled();
  });

  it("requests the shared renderer when a Markdown Mermaid block exists", async () => {
    const rendered = render(
      <MarkdownPreview
        baseRel="Notes/diagram.md"
        doc={{
          title: null,
          tags: [],
          html: '<div class="mermaid-block" data-idx="0"></div>',
          mermaid: ["graph TD; A-->B;"],
        }}
        onNavigate={() => undefined}
      />,
    );

    await waitFor(() => expect(getMermaidRendererMock).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(renderMock).toHaveBeenCalledWith(expect.any(String), "graph TD; A-->B;"));
    expect(rendered.container.querySelector(".preview-body")?.innerHTML).toContain('data-rendered="good"');
  });

  it("keeps the last-good SVG and shows an error badge after a failed render", async () => {
    const first = {
      title: null,
      tags: [],
      html: '<div class="mermaid-block" data-idx="0"></div>',
      mermaid: ["graph TD; A-->B;"],
    };
    const second = { ...first, mermaid: ["invalid"] };
    const rendered = render(
      <MarkdownPreview baseRel="Notes/diagram.md" doc={first} onNavigate={() => undefined} />,
    );

    await waitFor(() => expect(rendered.container.querySelector(".preview-body")?.innerHTML).toContain('data-rendered="good"'));
    renderMock.mockRejectedValueOnce(new Error("syntax error"));
    rendered.rerender(
      <MarkdownPreview baseRel="Notes/diagram.md" doc={second} onNavigate={() => undefined} />,
    );

    await waitFor(() => expect(rendered.container.querySelector(".mermaid-error-badge")).toBeTruthy());
    expect(rendered.container.querySelector(".preview-body")?.innerHTML).toContain('data-rendered="good"');
  });

  it("does not apply a stale render after the document changes", async () => {
    let resolveRender!: (result: { svg: string }) => void;
    renderMock.mockImplementationOnce(() => new Promise((resolve) => {
      resolveRender = resolve;
    }));
    const first = {
      title: null,
      tags: [],
      html: '<div class="mermaid-block" data-idx="0"></div>',
      mermaid: ["old"],
    };
    const rendered = render(
      <MarkdownPreview baseRel="Notes/diagram.md" doc={first} onNavigate={() => undefined} />,
    );
    await waitFor(() => expect(renderMock).toHaveBeenCalledTimes(1));

    rendered.rerender(
      <MarkdownPreview
        baseRel="Notes/diagram.md"
        doc={{ title: null, tags: [], html: "<p>new</p>", mermaid: [] }}
        onNavigate={() => undefined}
      />,
    );
    resolveRender({ svg: '<svg data-rendered="stale"></svg>' });

    await waitFor(() => expect(rendered.container.querySelector(".preview-body")?.textContent).toBe("new"));
    expect(rendered.container.querySelector(".preview-body")?.innerHTML).not.toContain("stale");
  });
});
