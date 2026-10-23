// 자동 보안 검토(becfd8d1): 검토 전 프로젝트 파일은 송신·실행·환경 적용에 쓰지 않는다(07b 34행).
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it } from "vitest";
import type { CollectionRoot } from "../../../rpc/gen/rpc";
import { ApiWorkspace } from "./ApiWorkspace";
import { ApiBackendProvider, SendFailure } from "./backend";
import { EnvEditor, resetEnvDrafts } from "./EnvEditor";
import { ImportDialog } from "./ImportDialog";
import { planImport } from "./importPlan";
import { RunnerView } from "./RunnerView";
import { useApiTabs } from "./store";
import { FakeBackend } from "./test-backend";

const personal = { kind: "personal" } as const;
const project = { kind: "project", projectId: "p1" } as const;
const requestText = (headers: { name: string; value: string }[], extra: Record<string, unknown> = {}) =>
  `${JSON.stringify(
    {
      version: 1,
      name: "A",
      request: {
        method: "GET",
        url: "https://x.test/a",
        query: [],
        headers: headers.map((h, i) => ({ id: `h${i}`, enabled: true, ...h })),
        cookies: [],
        body: { kind: "none" },
        auth: { kind: "none" },
        tls: { verify: true, caPath: null, clientCertPath: null, clientKeyPath: null },
        timeoutMs: 30_000,
        followRedirects: true,
      },
      assertions: [],
      captures: [],
      ...extra,
    },
    null,
    2,
  )}\n`;
const envText = `${JSON.stringify({ version: 1, name: "dev", variables: [{ name: "x", value: "{{secret:GH}}" }] }, null, 2)}\n`;

afterEach(() => {
  cleanup();
  resetEnvDrafts();
  useApiTabs.getState().reset();
});

function openTab(backend: FakeBackend, root: CollectionRoot, text: string, reviewed: boolean) {
  const digest = backend.seed(root, "a.request.json", text, "d1");
  if (reviewed) backend.reviewed.add(`${root.kind === "project" ? root.projectId : "~"}:a.request.json@${digest}`);
  act(() => {
    useApiTabs.getState().openFile(root, "a.request.json", text, digest, reviewed);
  });
}

it("unreviewed_project_file_is_not_sent_until_review_is_confirmed", async () => {
  const user = userEvent.setup();
  const backend = new FakeBackend();
  openTab(backend, project, requestText([{ name: "Authorization", value: "Bearer {{secret:GH}}" }]), false);
  render(
    <ApiBackendProvider backend={backend}>
      <ApiWorkspace />
    </ApiBackendProvider>,
  );
  await user.click(screen.getByRole("button", { name: "보내기" }));
  let dialog = await screen.findByRole("dialog", { name: "검토하고 보낼까요?" });
  expect(backend.send).not.toHaveBeenCalled();
  await user.click(within(dialog).getByRole("button", { name: "취소" }));
  expect(backend.send).not.toHaveBeenCalled();
  expect(backend.review).not.toHaveBeenCalled();

  // Ctrl+Enter도 같은 확인을 거친다.
  await user.click(screen.getByLabelText("URL"));
  await user.keyboard("{Control>}{Enter}{/Control}");
  dialog = await screen.findByRole("dialog", { name: "검토하고 보낼까요?" });
  expect(backend.send).not.toHaveBeenCalled();
  await user.click(within(dialog).getByRole("button", { name: "검토하고 보내기" }));
  await waitFor(() => expect(backend.send).toHaveBeenCalledOnce());
  expect(backend.review).toHaveBeenCalledOnce();
  expect(backend.review.mock.calls[0]).toEqual([project, "a.request.json", "d1"]);
  // 검토 기록이 송신보다 먼저.
  expect(backend.review.mock.invocationCallOrder[0]!).toBeLessThan(backend.send.mock.invocationCallOrder[0]!);
  // 검토한 뒤에는 다시 묻지 않는다.
  await waitFor(() => expect(screen.getByRole("button", { name: "보내기" })).toBeEnabled());
  await user.click(screen.getByRole("button", { name: "보내기" }));
  await waitFor(() => expect(backend.send).toHaveBeenCalledTimes(2));
  expect(backend.review).toHaveBeenCalledOnce();
});

it("review_failure_does_not_send", async () => {
  const user = userEvent.setup();
  const backend = new FakeBackend();
  openTab(backend, project, requestText([{ name: "Authorization", value: "Bearer {{secret:GH}}" }]), false);
  backend.review.mockRejectedValueOnce(new SendFailure("internal"));
  render(
    <ApiBackendProvider backend={backend}>
      <ApiWorkspace />
    </ApiBackendProvider>,
  );
  await user.click(screen.getByRole("button", { name: "보내기" }));
  const dialog = await screen.findByRole("dialog", { name: "검토하고 보낼까요?" });
  await user.click(within(dialog).getByRole("button", { name: "검토하고 보내기" }));
  await waitFor(() => expect(backend.review).toHaveBeenCalledOnce());
  await waitFor(() => expect(useApiTabs.getState().tabs[0]!.notice).toBeTruthy());
  expect(backend.send).not.toHaveBeenCalled();
  expect(useApiTabs.getState().tabs[0]!.reviewed).toBe(false);
});

it("file_reloaded_while_recording_review_is_not_sent", async () => {
  const user = userEvent.setup();
  const backend = new FakeBackend();
  openTab(backend, project, requestText([{ name: "Authorization", value: "Bearer {{secret:GH}}" }]), false);
  let finish!: () => void;
  backend.review.mockImplementationOnce(
    (root, path, digest) =>
      new Promise<void>((resolve) => {
        finish = () => {
          backend.reviewed.add(`${root.kind === "project" ? root.projectId : "~"}:${path}@${digest}`);
          resolve();
        };
      }),
  );
  render(
    <ApiBackendProvider backend={backend}>
      <ApiWorkspace />
    </ApiBackendProvider>,
  );
  await user.click(screen.getByRole("button", { name: "보내기" }));
  const dialog = await screen.findByRole("dialog", { name: "검토하고 보낼까요?" });
  await user.click(within(dialog).getByRole("button", { name: "검토하고 보내기" }));
  await waitFor(() => expect(backend.review).toHaveBeenCalledOnce());
  // 검토(d1)를 기록하는 동안 [다시 읽기]로 탭이 d2(다른 내용)가 된다.
  const tab = useApiTabs.getState().tabs[0]!;
  const changed = requestText([{ name: "Authorization", value: "Bearer {{secret:OTHER}}" }]);
  act(() => {
    useApiTabs.getState().reload(tab.id, { text: changed, digest: "d2", reviewed: false });
  });
  await act(async () => finish());
  await waitFor(() => expect(useApiTabs.getState().tabs[0]!.notice).toBeTruthy());
  expect(backend.send).not.toHaveBeenCalled();
  expect(useApiTabs.getState().tabs[0]!.reviewed).toBe(false);
});

it("file_marked_for_secret_review_is_not_sent_without_confirmation", async () => {
  const user = userEvent.setup();
  const backend = new FakeBackend();
  openTab(backend, personal, requestText([{ name: "X-Trace", value: "1" }], { requiresSecretReview: true }), true);
  render(
    <ApiBackendProvider backend={backend}>
      <ApiWorkspace />
    </ApiBackendProvider>,
  );
  await user.click(screen.getByRole("button", { name: "보내기" }));
  const dialog = await screen.findByRole("dialog", { name: "검토하고 보낼까요?" });
  expect(backend.send).not.toHaveBeenCalled();
  await user.click(within(dialog).getByRole("button", { name: "검토하고 보내기" }));
  await waitFor(() => expect(backend.send).toHaveBeenCalledOnce());
  // 개인 루트는 digest 검토 기록이 없다.
  expect(backend.review).not.toHaveBeenCalled();
});

it("reviewed_file_is_sent_without_asking", async () => {
  const user = userEvent.setup();
  const backend = new FakeBackend();
  openTab(backend, project, requestText([{ name: "Authorization", value: "Bearer {{secret:GH}}" }]), true);
  render(
    <ApiBackendProvider backend={backend}>
      <ApiWorkspace />
    </ApiBackendProvider>,
  );
  await user.click(screen.getByRole("button", { name: "보내기" }));
  await waitFor(() => expect(backend.send).toHaveBeenCalledOnce());
  expect(screen.queryByRole("dialog", { name: "검토하고 보낼까요?" })).toBeNull();
});

const har = (header: string) =>
  JSON.stringify({
    log: {
      entries: [
        {
          request: {
            method: "GET",
            url: "https://x.test/one",
            headers: [{ name: "X-Key", value: header }],
            queryString: [],
          },
        },
      ],
    },
  });

async function importInto(user: ReturnType<typeof userEvent.setup>, backend: FakeBackend, text: string) {
  render(
    <ApiBackendProvider backend={backend}>
      <ImportDialog
        open
        onOpenChange={() => {}}
        roots={[
          { id: "p1", label: "프로젝트", root: project },
          { id: "personal", label: "개인 컬렉션", root: personal },
        ]}
      />
    </ApiBackendProvider>,
  );
  const area = screen.getByLabelText("가져올 내용");
  await user.click(area);
  await user.paste(text);
  await user.click(screen.getByRole("button", { name: "미리보기" }));
  await screen.findByRole("list", { name: "만들 파일" });
  await user.click(screen.getByRole("button", { name: "가져오기" }));
}

it("import_does_not_record_review_for_files_with_secret_references", async () => {
  const user = userEvent.setup();
  const backend = new FakeBackend();
  await importInto(user, backend, har("{{secret:GH}}"));
  await waitFor(() => expect(backend.write).toHaveBeenCalledOnce());
  const [, path, text] = backend.write.mock.calls[0]!;
  expect(text).toContain("{{secret:GH}}");
  await screen.findByText(/요청 1개를 가져왔습니다/);
  expect(backend.review).not.toHaveBeenCalled();
  // 기준은 데몬이 돌려주는 검토 상태다(FakeBackend.write는 데몬 쓰기와 같게 동작).
  const read = await backend.read(project, path);
  expect(read.reviewed).toBe(false);
  // 가져온 파일을 열어 보내면 확인 창을 거친다.
  cleanup();
  act(() => {
    useApiTabs.getState().openFile(project, path, read.text, read.digest, read.reviewed);
  });
  render(
    <ApiBackendProvider backend={backend}>
      <ApiWorkspace />
    </ApiBackendProvider>,
  );
  await user.click(screen.getByRole("button", { name: "보내기" }));
  await screen.findByRole("dialog", { name: "검토하고 보낼까요?" });
  expect(backend.send).not.toHaveBeenCalled();
});

it("imported_environments_keep_secret_references_and_stay_unreviewed", async () => {
  // Postman·Insomnia 환경 변환은 `{{secret:…}}` 값을 그대로 남긴다.
  const postman = JSON.stringify({
    info: { name: "Demo", schema: "https://schema.getpostman.com/json/collection/v2.1.0/collection.json" },
    item: [{ name: "One", request: { method: "GET", url: "https://x.test/one" } }],
    variable: [{ key: "x", value: "{{secret:GH}}" }],
  });
  const insomnia = JSON.stringify({
    _type: "export",
    __export_format: 4,
    resources: [
      { _id: "wrk_1", _type: "workspace", name: "W" },
      { _id: "env_1", _type: "environment", parentId: "wrk_1", name: "Base Environment", data: { x: "{{secret:GH}}" } },
    ],
  });
  for (const source of [postman, insomnia]) {
    const env = planImport(source).files.find((file) => file.kind === "environment")!;
    expect(JSON.parse(env.text(project)).variables).toEqual([{ name: "x", value: "{{secret:GH}}" }]);
  }
  const user = userEvent.setup();
  const backend = new FakeBackend();
  await importInto(user, backend, postman);
  await screen.findByText(/가져왔습니다/);
  const envWrite = backend.write.mock.calls.find((call) => call[1].startsWith("env/"))!;
  expect(envWrite[2]).toContain("{{secret:GH}}");
  expect((await backend.read(project, envWrite[1])).reviewed).toBe(false);
  expect(backend.review).not.toHaveBeenCalled();
});

it("keeping_files_after_a_failed_import_does_not_record_review", async () => {
  const user = userEvent.setup();
  const backend = new FakeBackend();
  const two = JSON.stringify({
    log: {
      entries: ["one", "two"].map((name) => ({
        request: { method: "GET", url: `https://x.test/${name}`, headers: [], queryString: [] },
      })),
    },
  });
  backend.failWrite.add("x.test/GET -two.request.json");
  await importInto(user, backend, two);
  const confirm = await screen.findByRole("dialog", { name: "가져오기를 끝내지 못했습니다" });
  await user.click(within(confirm).getByRole("button", { name: "파일 남기기" }));
  await screen.findByText(/남긴 파일: x\.test\/GET -one\.request\.json/);
  expect(backend.review).not.toHaveBeenCalled();
  expect((await backend.read(project, "x.test/GET -one.request.json")).reviewed).toBe(false);
});

async function loadProjectEnv(user: ReturnType<typeof userEvent.setup>, backend: FakeBackend) {
  backend.seed(project, "env/dev.json", envText, "e1");
  render(
    <ApiBackendProvider backend={backend}>
      <EnvEditor root={project} />
      <ApiWorkspace />
    </ApiBackendProvider>,
  );
  await screen.findByRole("option", { name: "dev" });
  await user.selectOptions(screen.getByLabelText("환경"), "env/dev.json");
  await waitFor(() => expect(screen.getByLabelText("변수 1 값")).toHaveValue("{{secret:GH}}"));
}

it("unreviewed_project_environment_is_not_used_until_review_is_confirmed", async () => {
  const user = userEvent.setup();
  const backend = new FakeBackend();
  openTab(backend, personal, requestText([{ name: "X-Key", value: "{{x}}" }]), true);
  await loadProjectEnv(user, backend);
  const banner = screen.getByText(/검토하지 않은 프로젝트 환경입니다/);
  await user.click(screen.getByRole("button", { name: "보내기" }));
  await screen.findByText(/환경 파일을 검토한 뒤 보낼 수 있습니다/);
  expect(backend.send).not.toHaveBeenCalled();

  await user.click(within(banner.closest("[role]") ?? document.body).getByRole("button", { name: "검토하고 사용" }));
  await waitFor(() => expect(backend.review).toHaveBeenCalledOnce());
  expect(backend.review.mock.calls[0]).toEqual([project, "env/dev.json", "e1"]);
  await waitFor(() => expect(screen.queryByText(/검토하지 않은 프로젝트 환경입니다/)).toBeNull());
  await user.click(screen.getByRole("button", { name: "보내기" }));
  await waitFor(() => expect(backend.send).toHaveBeenCalledOnce());
  expect(backend.send.mock.calls[0]![0].env.variables).toEqual([
    { name: "x", value: "{{secret:GH}}", sensitive: true, origin: "environment" },
  ]);
});

it("runner_marks_entries_using_an_unreviewed_environment", async () => {
  const user = userEvent.setup();
  const backend = new FakeBackend();
  await loadProjectEnv(user, backend);
  cleanup();
  const uses = requestText([{ name: "X-Key", value: "{{x}}" }]);
  const plain = requestText([{ name: "X-Trace", value: "1" }])
    .replace('"name": "A"', '"name": "B"')
    .replace("x.test/a", "x.test/b");
  backend.seed(personal, "a.request.json", uses);
  backend.seed(personal, "b.request.json", plain);
  render(
    <ApiBackendProvider backend={backend}>
      <RunnerView root={personal} />
    </ApiBackendProvider>,
  );
  await waitFor(() =>
    expect(within(screen.getByRole("list", { name: "실행 순서" })).getAllByRole("listitem")).toHaveLength(2),
  );
  await user.click(screen.getByRole("button", { name: "실행" }));
  const results = await screen.findByRole("table", { name: "실행 결과" });
  await waitFor(() => expect(within(results).getAllByRole("row")).toHaveLength(3));
  expect(within(results).getByRole("row", { name: "A" })).toHaveTextContent("비밀 검토가 필요합니다");
  expect(within(results).getByRole("row", { name: "B" })).toHaveTextContent("통과");
  expect(backend.send).toHaveBeenCalledOnce();
  expect(backend.send.mock.calls[0]![0].snapshot.url).toBe("https://x.test/b");
  expect(backend.send.mock.calls[0]![0].env.variables).toEqual([]);
});
