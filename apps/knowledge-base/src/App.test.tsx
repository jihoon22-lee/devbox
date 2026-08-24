import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import App from "./App";

/** listTree가 .md 파일과 .png 파일을 하나씩 반환하도록 ./api를 모킹한다. */
vi.mock("./api", () => {
  const TREE = [
    { path: "note.md", is_dir: false },
    { path: "image.png", is_dir: false },
  ];
  return {
    listTree: vi.fn(async () => TREE),
    listTags: vi.fn(async () => [] as string[]),
    readFile: vi.fn(async (path: string) => (path.endsWith(".md") ? "# Hello" : "binary-content")),
    openInboundNote: vi.fn(async () => ({ path: "note.md", content: "# Hello" })),
    takePendingOpen: vi.fn(async () => null),
    onOpenRequest: vi.fn(async () => () => undefined),
    writeFile: vi.fn(async () => undefined),
    createFile: vi.fn(async () => undefined),
    renameFile: vi.fn(async () => undefined),
    deleteFile: vi.fn(async () => undefined),
    searchDocs: vi.fn(async () => []),
    dailyNote: vi.fn(async () => ["daily.md", "# Today"] as [string, string]),
    renderMarkdown: vi.fn(async () => ({ title: null, tags: [], html: "<p>rendered</p>", mermaid: [] })),
    onDocsChanged: vi.fn(async () => () => undefined),
  };
});

afterEach(() => cleanup());

describe("knowledge-base App — 모드 토글 & 프리뷰 비활성화", () => {
  it(".md 파일을 열면 분할/프리뷰 버튼이 활성화된다", async () => {
    render(<App />);

    fireEvent.click(await screen.findByText("note.md"));

    const previewBtn = await screen.findByRole("button", { name: "프리뷰" });
    const splitBtn = await screen.findByRole("button", { name: "분할" });
    expect(previewBtn).not.toBeDisabled();
    expect(splitBtn).not.toBeDisabled();
  });

  it(".md가 아닌 파일을 열면 분할/프리뷰 버튼이 비활성화된다", async () => {
    render(<App />);

    fireEvent.click(await screen.findByText("image.png"));

    const previewBtn = await screen.findByRole("button", { name: "프리뷰" });
    const splitBtn = await screen.findByRole("button", { name: "분할" });
    expect(previewBtn).toBeDisabled();
    expect(splitBtn).toBeDisabled();
  });

  it("편집 -> 분할 -> 프리뷰 전환에 따라 editor-body의 mode-* 클래스가 바뀐다", async () => {
    const { container } = render(<App />);
    const body = () => container.querySelector(".editor-body");

    // openFile은 readFile을 await하므로 클릭 직후엔 아직 상태가 안 바뀌어 있을 수 있다 —
    // waitFor로 비동기 상태 반영을 기다린 뒤 단언한다.
    fireEvent.click(await screen.findByText("note.md"));
    await waitFor(() => expect(body()?.className).toContain("mode-edit"));

    fireEvent.click(screen.getByRole("button", { name: "분할" }));
    await waitFor(() => expect(body()?.className).toContain("mode-split"));

    fireEvent.click(screen.getByRole("button", { name: "프리뷰" }));
    await waitFor(() => expect(body()?.className).toContain("mode-preview"));

    fireEvent.click(screen.getByRole("button", { name: "편집" }));
    await waitFor(() => expect(body()?.className).toContain("mode-edit"));
  });

  it("분할/프리뷰 모드로 본 뒤 .md가 아닌 파일로 전환하면 편집 모드로 강제 복귀한다", async () => {
    const { container } = render(<App />);
    const body = () => container.querySelector(".editor-body");

    fireEvent.click(await screen.findByText("note.md"));
    await waitFor(() => expect(body()?.className).toContain("mode-edit"));

    fireEvent.click(screen.getByRole("button", { name: "프리뷰" }));
    await waitFor(() => expect(body()?.className).toContain("mode-preview"));

    fireEvent.click(await screen.findByText("image.png"));
    await waitFor(() => expect(body()?.className).toContain("mode-edit"));
  });
});
