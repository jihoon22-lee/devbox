import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const fixture = vi.hoisted(() => ({ render: vi.fn(), load: vi.fn() }));
vi.mock("@devbox/mermaid-renderer", () => ({ getMermaidRenderer: fixture.load }));
import { MarkdownBody } from "./MarkdownBody";

afterEach(() => {
  cleanup();
  fixture.render.mockReset();
  fixture.load.mockReset();
});
const html = '<h2>Intro</h2><div class="mermaid-block" data-idx="0"></div><a href="notes/b.md">b</a>';
function renderer() {
  fixture.load.mockResolvedValue({ render: fixture.render });
}

describe("MarkdownBody", () => {
  it("renders native-sanitized HTML, preserves heading IDs and lazily loads diagrams", async () => {
    renderer();
    fixture.render.mockResolvedValue({ svg: "<svg id='ok'></svg>" });
    const view = render(
      <MarkdownBody
        html="<h2>Intro</h2><h2>Intro</h2><h2 id='owned'>Intro</h2>"
        mermaid={[]}
        docKey="a"
        idPrefix="test"
        assignHeadingIds
      />,
    );
    expect(screen.getAllByRole("heading").map((node) => node.id)).toEqual(["intro", "intro-1", "owned"]);
    expect(fixture.load).not.toHaveBeenCalled();
    view.rerender(
      <MarkdownBody html={html} mermaid={["graph TD; A-->B"]} docKey="a" idPrefix="test" assignHeadingIds />,
    );
    await waitFor(() => expect(view.container.querySelector("svg#ok")).not.toBeNull());
    expect(screen.getByRole("heading", { name: "Intro" }).id).toBe("intro");
  });

  it("keeps the last good diagram and shows an error badge after a broken edit", async () => {
    renderer();
    fixture.render.mockResolvedValueOnce({ svg: "<svg id='good'></svg>" }).mockRejectedValueOnce(new Error("syntax"));
    const view = render(<MarkdownBody html={html} mermaid={["valid"]} docKey="a" idPrefix="test" />);
    await waitFor(() => expect(view.container.querySelector("svg#good")).not.toBeNull());
    view.rerender(<MarkdownBody html={html} mermaid={["invalid"]} docKey="a" idPrefix="test" />);
    await waitFor(() => expect(view.container.textContent).toContain("⚠ 구문 오류"));
    expect(view.container.querySelector("svg#good")).not.toBeNull();
  });

  it("does not apply a late diagram after changing documents", async () => {
    renderer();
    let finish!: (value: { svg: string }) => void;
    fixture.render.mockReturnValueOnce(
      new Promise((done) => {
        finish = done;
      }),
    );
    const view = render(<MarkdownBody html={html} mermaid={["old"]} docKey="a" idPrefix="test" />);
    await waitFor(() => expect(fixture.render).toHaveBeenCalledTimes(1));
    view.rerender(<MarkdownBody html="<p>other</p>" mermaid={[]} docKey="b" idPrefix="test" />);
    await act(async () => finish({ svg: "<svg id='late'></svg>" }));
    expect(view.container.querySelector("svg#late")).toBeNull();
  });

  it("does not reuse another document's cached SVG even when HTML is identical", async () => {
    renderer();
    fixture.render.mockResolvedValueOnce({ svg: "<svg id='old'></svg>" }).mockRejectedValueOnce(new Error("syntax"));
    const view = render(<MarkdownBody html={html} mermaid={["valid"]} docKey="a" idPrefix="test" assignHeadingIds />);
    await waitFor(() => expect(view.container.querySelector("svg#old")).not.toBeNull());
    view.rerender(<MarkdownBody html={html} mermaid={["invalid"]} docKey="b" idPrefix="test" assignHeadingIds />);
    await waitFor(() => expect(view.container.textContent).toContain("⚠ 구문 오류"));
    expect(view.container.querySelector("svg#old")).toBeNull();
    expect(screen.getByRole("heading", { name: "Intro" }).id).toBe("intro");
  });

  it("ignores an older edit and an unmounted render", async () => {
    renderer();
    const finishes: Array<(value: { svg: string }) => void> = [];
    fixture.render.mockImplementation(() => new Promise((done) => finishes.push(done)));
    const view = render(<MarkdownBody html={html} mermaid={["old"]} docKey="a" idPrefix="test" />);
    await waitFor(() => expect(finishes).toHaveLength(1));
    view.rerender(<MarkdownBody html={html} mermaid={["new"]} docKey="a" idPrefix="test" />);
    await waitFor(() => expect(finishes).toHaveLength(2));
    await act(async () => finishes[1]({ svg: "<svg id='new'></svg>" }));
    await act(async () => finishes[0]({ svg: "<svg id='old'></svg>" }));
    expect(view.container.querySelector("svg#new")).not.toBeNull();
    expect(view.container.querySelector("svg#old")).toBeNull();
    view.rerender(<MarkdownBody html={html} mermaid={["pending"]} docKey="a" idPrefix="test" />);
    await waitFor(() => expect(finishes).toHaveLength(3));
    view.unmount();
    await act(async () => finishes[2]({ svg: "<svg id='unmounted'></svg>" }));
    expect(view.container.innerHTML).toBe("");
  });

  it("reports a renderer load failure and delegates links to the owner", async () => {
    fixture.load.mockRejectedValue(new Error("offline"));
    const onClick = vi.fn();
    render(<MarkdownBody html={html} mermaid={["diagram"]} docKey="a" idPrefix="test" onClick={onClick} />);
    await screen.findByText("⚠ 구문 오류");
    act(() => screen.getByText("b").click());
    expect(onClick).toHaveBeenCalledTimes(1);
  });
});
