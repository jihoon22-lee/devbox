import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it } from "vitest";
import type { CollectionRoot } from "../../../rpc/gen/rpc";
import { ApiBackendProvider } from "./backend";
import { ImportDialog } from "./ImportDialog";
import { FakeBackend } from "./test-backend";

afterEach(cleanup);
const personal = { kind: "personal" } as const;
const project = { kind: "project", projectId: "p1" } as const;
const har = JSON.stringify({
  log: {
    entries: [
      { request: { method: "GET", url: "https://x.test/one", headers: [], queryString: [] } },
      { request: { method: "GET", url: "https://x.test/two", headers: [], queryString: [] } },
    ],
  },
});
function setup(
  backend: FakeBackend,
  roots: { id: string; label: string; root: CollectionRoot }[] = [
    { id: "personal", label: "개인 컬렉션", root: personal },
  ],
) {
  render(
    <ApiBackendProvider backend={backend}>
      <ImportDialog open onOpenChange={() => {}} roots={roots} />
    </ApiBackendProvider>,
  );
}
async function preview() {
  fireEvent.change(screen.getByLabelText("가져올 내용"), { target: { value: har } });
  await userEvent.click(screen.getByRole("button", { name: "미리보기" }));
  return screen.findByRole("list", { name: "만들 파일" });
}

it("a_failure_in_the_middle_removes_only_the_files_this_import_created", async () => {
  const user = userEvent.setup();
  const backend = new FakeBackend();
  backend.seed(personal, "keep.request.json", "keep");
  setup(backend);
  const files = await preview();
  const paths = within(files)
    .getAllByRole("listitem")
    .map((item) => item.textContent ?? "");
  expect(paths).toHaveLength(2);
  backend.failWrite.add(paths[1]!);
  await user.click(screen.getByRole("button", { name: "가져오기" }));
  const confirm = await screen.findByRole("dialog", { name: "가져오기를 끝내지 못했습니다" });
  expect(confirm).toHaveTextContent("이번에 만든 파일 1개를 지울까요?");
  // 확인 창은 한 번만(삭제는 확인 정책 always), 지우기 전에는 아무것도 지우지 않는다.
  expect(backend.remove).not.toHaveBeenCalled();
  await user.click(within(confirm).getByRole("button", { name: "만든 파일 지우기" }));
  await waitFor(() => expect(backend.remove).toHaveBeenCalledOnce());
  expect(backend.remove.mock.calls[0]!.slice(0, 2)).toEqual([personal, paths[0]]);
  expect(await screen.findByText("만든 파일 1개를 지웠습니다.")).toBeInTheDocument();
  expect([...backend.files.keys()]).toEqual(["~:keep.request.json"]);
});

it("keeping_created_files_lists_them_and_project_imports_record_review", async () => {
  const user = userEvent.setup();
  const backend = new FakeBackend();
  setup(backend, [
    { id: "personal", label: "개인 컬렉션", root: personal },
    { id: "p1", label: "devbox 프로젝트", root: project },
  ]);
  await preview();
  await user.selectOptions(screen.getByLabelText("저장할 곳"), "p1");
  await user.click(screen.getByRole("button", { name: "가져오기" }));
  await waitFor(() => expect(backend.write).toHaveBeenCalledTimes(2));
  expect(backend.write.mock.calls.every((call) => call[0] === project && call[3].kind === "absent")).toBe(true);
  // 사용자가 미리보기로 본 내용이므로 쓴 digest로 검토를 함께 기록한다.
  await waitFor(() => expect(backend.review).toHaveBeenCalledTimes(2));
  expect(backend.review.mock.calls.map((call) => [call[0], call[1]])).toEqual(
    backend.write.mock.calls.map((call) => [project, call[1]]),
  );
  expect(await screen.findByText("요청 2개를 가져왔습니다.")).toBeInTheDocument();
  // 다시 가져오면 같은 이름이 있어 실패 → 남기기를 고르면 남긴 파일을 보여 준다.
  backend.failWrite.add("x.test/GET -two.request.json");
  backend.files.clear();
  await preview();
  await user.click(screen.getByRole("button", { name: "가져오기" }));
  const confirm = await screen.findByRole("dialog", { name: "가져오기를 끝내지 못했습니다" });
  await user.click(within(confirm).getByRole("button", { name: "파일 남기기" }));
  expect(await screen.findByText(/남긴 파일: x\.test\/GET -one\.request\.json/)).toBeInTheDocument();
  expect(backend.remove).not.toHaveBeenCalled();
});
