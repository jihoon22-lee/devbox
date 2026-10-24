import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
import { ApiWorkspace } from "./ApiWorkspace";
import { ApiBackendProvider, SendFailure } from "./backend";
import { CollectionTree } from "./CollectionTree";
import { parseRequestFile, serializeRequestFile } from "./model/requestFile";
import type { StoredRequest } from "./model/stored";
import { useApiTabs } from "./store";
import { FakeBackend, textResponse } from "./test-backend";

const personal = { kind: "personal" } as const;
function stored(extra: Partial<StoredRequest> = {}): StoredRequest {
  return {
    method: "GET",
    url: "https://x.test/a",
    query: [],
    headers: [],
    cookies: [],
    body: { kind: "none" },
    auth: { kind: "none" },
    tls: { verify: true, caPath: null, clientCertPath: null, clientKeyPath: null },
    timeoutMs: 30_000,
    followRedirects: true,
    ...extra,
  };
}
function open(backend: FakeBackend, request = stored(), extra: Record<string, unknown> = {}) {
  const text = `${JSON.stringify({ version: 1, name: "A", request, assertions: [], captures: [], ...extra }, null, 2)}\n`;
  const digest = backend.seed(personal, "a.request.json", text, "d1");
  act(() => {
    useApiTabs.getState().openFile(personal, "a.request.json", text, digest);
  });
}
function setup(backend = new FakeBackend()) {
  render(
    <ApiBackendProvider backend={backend}>
      <CollectionTree root={personal} />
      <ApiWorkspace />
    </ApiBackendProvider>,
  );
  return backend;
}
afterEach(() => {
  cleanup();
  useApiTabs.getState().reset();
});
const dirtyMarks = () => screen.queryAllByLabelText("저장하지 않은 변경");

describe("request editor", () => {
  it("save_and_save_as_new_are_distinct", async () => {
    const user = userEvent.setup();
    const backend = new FakeBackend();
    open(backend);
    setup(backend);
    await screen.findByRole("treeitem", { name: /^a/ });
    await user.type(screen.getByLabelText("URL"), "/b");
    // 탭과 트리 모두에 dirty 점.
    await waitFor(() => expect(dirtyMarks()).toHaveLength(2));
    await user.click(screen.getByRole("button", { name: "저장" }));
    await waitFor(() => expect(backend.write).toHaveBeenCalledOnce());
    expect(backend.write.mock.calls[0]!.slice(0, 2)).toEqual([personal, "a.request.json"]);
    expect(backend.write.mock.calls[0]![3]).toEqual({ kind: "digest", digest: "d1" });
    await waitFor(() => expect(dirtyMarks()).toHaveLength(0));
    expect(JSON.parse(backend.text(personal, "a.request.json")!).request.url).toBe("https://x.test/a/b");

    await user.type(screen.getByLabelText("URL"), "/c");
    await user.click(screen.getByRole("button", { name: "새 요청으로 저장" }));
    const dialog = await screen.findByRole("dialog", { name: "새 요청으로 저장" });
    await user.clear(within(dialog).getByLabelText("요청 이름"));
    await user.type(within(dialog).getByLabelText("요청 이름"), "B copy");
    await user.click(within(dialog).getByRole("button", { name: "저장" }));
    await waitFor(() => expect(backend.write).toHaveBeenCalledTimes(2));
    expect(backend.write.mock.calls[1]![1]).toBe("B copy.request.json");
    expect(backend.write.mock.calls[1]![3]).toEqual({ kind: "absent" });
    expect(backend.files.size).toBe(2);
    // 원래 파일은 그대로, 새 파일에 고친 내용.
    expect(JSON.parse(backend.text(personal, "a.request.json")!).request.url).toBe("https://x.test/a/b");
    expect(JSON.parse(backend.text(personal, "B copy.request.json")!)).toMatchObject({
      name: "B copy",
      request: { url: "https://x.test/a/b/c" },
    });
    await waitFor(() => expect(dirtyMarks()).toHaveLength(0));
    expect(screen.getByRole("tab", { name: /B copy/ })).toHaveAttribute("aria-selected", "true");
  });

  it("invalid_header_error_is_shown_under_its_row", async () => {
    const user = userEvent.setup();
    const backend = new FakeBackend();
    backend.reply = async () => {
      throw new SendFailure("api.invalid_header", { index: 1, field: "name", reason: "token" });
    };
    open(
      backend,
      stored({
        headers: [
          { name: "X-Off", value: "1", enabled: false },
          { name: "X API Key", value: "1", enabled: true },
        ],
      }),
    );
    setup(backend);
    await user.click(screen.getByRole("tab", { name: "헤더" }));
    await user.click(screen.getByRole("button", { name: "보내기" }));
    const second = await screen.findByRole("group", { name: "헤더 2" });
    expect(await within(second).findByRole("alert")).toHaveTextContent("헤더 2행 이름이 올바르지 않습니다.");
    expect(within(screen.getByRole("group", { name: "헤더 1" })).queryByRole("alert")).toBeNull();
  });

  it("masked_secrets_are_announced_before_saving", async () => {
    const user = userEvent.setup();
    const backend = new FakeBackend();
    open(backend);
    setup(backend);
    await user.click(screen.getByRole("tab", { name: "헤더" }));
    await user.click(screen.getByRole("button", { name: "헤더 추가" }));
    await user.type(screen.getByLabelText("헤더 1 이름"), "Authorization");
    await user.type(screen.getByLabelText("헤더 1 값"), "Bearer typed-secret");
    await user.click(screen.getByRole("button", { name: "저장" }));
    const dialog = await screen.findByRole("dialog", { name: "직접 입력한 비밀 1개는 파일에 저장하지 않습니다" });
    expect(backend.write).not.toHaveBeenCalled();
    await user.click(within(dialog).getByRole("button", { name: "가리고 저장" }));
    await waitFor(() => expect(backend.write).toHaveBeenCalledOnce());
    const text = backend.text(personal, "a.request.json")!;
    expect(text).not.toContain("typed-secret");
    expect(JSON.parse(text).masked).toEqual(["request.headers.0.value"]);
  });

  it("move_to_secret_replaces_the_value_with_a_reference", async () => {
    const user = userEvent.setup();
    const backend = new FakeBackend();
    backend.secrets.set("A_AUTHORIZATION", "existing");
    open(backend, stored({ headers: [{ name: "Authorization", value: "", enabled: true }] }));
    setup(backend);
    await user.click(screen.getByRole("tab", { name: "헤더" }));
    await user.type(screen.getByLabelText("헤더 1 값"), "Bearer typed-secret");
    await user.click(screen.getByRole("button", { name: "저장" }));
    const dialog = await screen.findByRole("dialog", { name: "직접 입력한 비밀 1개는 파일에 저장하지 않습니다" });
    await user.click(within(dialog).getByRole("button", { name: "비밀로 옮기기" }));
    await waitFor(() => expect(backend.write).toHaveBeenCalledOnce());
    // 이미 있는 이름은 덮지 않는다.
    expect(backend.secrets.get("A_AUTHORIZATION")).toBe("existing");
    expect(backend.secrets.get("A_AUTHORIZATION_2")).toBe("Bearer typed-secret");
    const file = JSON.parse(backend.text(personal, "a.request.json")!);
    expect(file.request.headers[0].value).toBe("{{secret:A_AUTHORIZATION_2}}");
    expect(file).not.toHaveProperty("masked");
    expect(screen.getByLabelText("헤더 1 값")).toHaveValue("{{secret:A_AUTHORIZATION_2}}");
  });

  it("redacted_value_blocks_sending", async () => {
    const user = userEvent.setup();
    const backend = new FakeBackend();
    const text = serializeRequestFile(
      {
        version: 1,
        name: "A",
        request: stored({ headers: [{ name: "Authorization", value: "Bearer raw", enabled: true }] }),
        assertions: [],
        captures: [],
      },
      personal,
    ).text;
    expect(parseRequestFile(text).file.masked).toEqual(["request.headers.0.value"]);
    backend.seed(personal, "a.request.json", text, "d1");
    act(() => {
      useApiTabs.getState().openFile(personal, "a.request.json", text, "d1");
    });
    setup(backend);
    await user.click(screen.getByRole("button", { name: "보내기" }));
    expect(await screen.findByText("가린 값을 다시 입력해 주세요")).toBeInTheDocument();
    expect(backend.send).not.toHaveBeenCalled();
  });

  it("user_typed_redacted_text_is_not_blocked", async () => {
    const user = userEvent.setup();
    const backend = new FakeBackend();
    backend.reply = async () => textResponse('{"ok":true}');
    open(backend, stored({ headers: [{ name: "X-Note", value: "[REDACTED]", enabled: true }] }));
    setup(backend);
    await user.click(screen.getByRole("button", { name: "보내기" }));
    await waitFor(() => expect(backend.send).toHaveBeenCalledOnce());
    expect(backend.send.mock.calls[0]![0].snapshot.headers).toEqual([{ row: 0, name: "X-Note", value: "[REDACTED]" }]);
    expect(await screen.findByText("200 OK")).toBeInTheDocument();
  });
});
