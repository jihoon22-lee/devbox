import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { ImportDialog } from "./ImportDialog";
import { readImportFiles, pickCollectionFolder, readCollectionFolder } from "./api";
vi.mock("./api", () => ({ readImportFiles: vi.fn(), pickCollectionFolder: vi.fn(), readCollectionFolder: vi.fn() }));
afterEach(() => {
  cleanup();
  vi.resetAllMocks();
});
it("previews curl, applies the selection and offers undo", async () => {
  const undo = vi.fn().mockResolvedValue(undefined);
  const apply = vi.fn().mockResolvedValue(undo);
  const { container } = render(<ImportDialog onClose={() => {}} onApply={apply} />);
  fireEvent.change(screen.getByLabelText("curl 명령"), {
    target: { value: "curl -X POST https://api.example.com/users" },
  });
  fireEvent.click(screen.getByRole("button", { name: "미리 보기" }));
  expect(await screen.findByRole("checkbox", { name: /POST \/users/ })).toBeTruthy();
  await assertNoA11yViolations(container);
  fireEvent.click(screen.getByRole("button", { name: "선택한 1개 가져오기" }));
  await screen.findByText("1개를 가져왔습니다.");
  expect(apply.mock.calls[0][0].collections.collections).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: "되돌리기" }));
  await waitFor(() => expect(undo).toHaveBeenCalledOnce());
});
it("file parse failure never applies data", async () => {
  vi.mocked(readImportFiles).mockResolvedValue([{ name: "bad.json", relativePath: "bad.json", text: "not json" }]);
  const apply = vi.fn();
  render(<ImportDialog onClose={() => {}} onApply={apply} />);
  fireEvent.click(screen.getByRole("button", { name: "파일에서" }));
  fireEvent.click(screen.getByRole("button", { name: "파일 선택" }));
  await screen.findByText("가져올 수 없는 형식입니다.");
  expect(apply).not.toHaveBeenCalled();
});
it("groups file requests, excludes unchecked entries and imports environments only on request", async () => {
  vi.mocked(readImportFiles).mockResolvedValue([
    {
      name: "demo.bru",
      relativePath: "Ops/demo.bru",
      text: "meta {\n  name: Health\n}\nget {\n  url: https://x.test\n}\nheaders {\n  Authorization: Bearer secret\n}\n",
    },
    { name: "dev.bru", relativePath: "environments/dev.bru", text: "vars {\n  base: https://x.test\n}\n" },
    {
      name: "two.bru",
      relativePath: "Ops/two.bru",
      text: "meta {\n  name: Skip\n}\nget {\n  url: https://x.test/two\n}\n",
    },
  ]);
  const apply = vi.fn().mockResolvedValue(async () => {});
  const { container } = render(<ImportDialog onClose={() => {}} onApply={apply} />);
  fireEvent.click(screen.getByRole("button", { name: "파일에서" }));
  fireEvent.change(screen.getByLabelText("가져오기 형식"), { target: { value: "bruno" } });
  fireEvent.click(screen.getByRole("button", { name: "파일 선택" }));
  await screen.findByText("비밀 검토 필요");
  expect(screen.getByText("환경 1개")).toBeTruthy();
  fireEvent.click(screen.getByRole("checkbox", { name: /Skip/ }));
  await assertNoA11yViolations(container);
  fireEvent.click(screen.getByRole("button", { name: "선택한 1개 가져오기" }));
  await waitFor(() => expect(apply).toHaveBeenCalledOnce());
  expect(apply.mock.calls[0][0].collections.collections.map((item: { name: string }) => item.name)).toEqual(["Health"]);
  expect(apply.mock.calls[0][0].environments.environments).toEqual([]);
});

it("opens a granted collection folder into the existing preview", async () => {
  vi.mocked(pickCollectionFolder).mockResolvedValue({ grant_id: "folder-grant", name: "Demo" });
  vi.mocked(readCollectionFolder).mockResolvedValue([
    {
      name: "health.bru",
      relativePath: "Ops/health.bru",
      text: "meta {\n  name: Health\n}\nget {\n  url: https://x.test\n}\n",
    },
  ]);
  const { container } = render(<ImportDialog onClose={() => {}} onApply={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "폴더에서" }));
  fireEvent.click(screen.getByRole("button", { name: "폴더 선택" }));
  await screen.findByRole("checkbox", { name: /Health/ });
  expect(readCollectionFolder).toHaveBeenCalledWith("folder-grant");
  await assertNoA11yViolations(container);
});
