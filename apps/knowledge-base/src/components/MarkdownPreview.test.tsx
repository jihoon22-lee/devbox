import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import MarkdownPreview from "./MarkdownPreview";

vi.mock("../api", () => ({
  openExternal: vi.fn(async () => undefined),
}));

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
