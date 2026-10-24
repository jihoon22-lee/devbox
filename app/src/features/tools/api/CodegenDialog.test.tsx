import { invoke } from "@tauri-apps/api/core";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterAll, afterEach, beforeAll, expect, it, vi } from "vitest";
import { CodegenDialog } from "./CodegenDialog";
import { CODE_TARGETS } from "./lib/codegen";
import { emptyDraft, type RequestDraft, setBodyKind, updateBody } from "./model/draft";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
afterEach(cleanup);
const originalGeometry = ["getClientRects", "getBoundingClientRect"].map((name) =>
  Object.getOwnPropertyDescriptor(Range.prototype, name),
);
beforeAll(() => {
  Object.defineProperties(Range.prototype, {
    getClientRects: { configurable: true, value: () => [] },
    getBoundingClientRect: { configurable: true, value: () => new DOMRect() },
  });
});
afterAll(() =>
  ["getClientRects", "getBoundingClientRect"].forEach((name, index) => {
    const descriptor = originalGeometry[index];
    if (descriptor) Object.defineProperty(Range.prototype, name, descriptor);
    else Reflect.deleteProperty(Range.prototype, name);
  }),
);
const base = () => ({ ...emptyDraft(), method: "POST", url: "https://owned.invalid/upload" });
const privatePath = "/owned/PRIVATE_FILE_CANARY.pem";
const binary = () => updateBody(setBodyKind(base(), "binary"), "binary", { path: privatePath });
const encoded = () =>
  updateBody(setBodyKind(base(), "raw"), "raw", {
    text: "AP+A",
    encoding: "base64",
    contentType: "application/octet-stream",
  });
const view = (draft: RequestDraft) => <CodegenDialog open onOpenChange={() => {}} draft={draft} environment={[]} />;
const unsupported = [
  ["binary", binary, "파일 본문의 코드 생성은 아직 지원하지 않습니다"],
  ["raw-base64", encoded, "Base64 원시 본문의 코드 생성은 아직 지원하지 않습니다"],
  ...(["caPath", "clientCertPath", "clientKeyPath"] as const).map(
    (field) =>
      [
        field,
        () => ({ ...base(), tls: { ...base().tls, [field]: privatePath } }),
        "사용자 TLS 파일이 설정된 요청의 코드 생성은 아직 지원하지 않습니다",
      ] as const,
  ),
  [
    "CA and identity",
    () => ({
      ...base(),
      tls: { ...base().tls, caPath: privatePath, clientCertPath: privatePath, clientKeyPath: privatePath },
    }),
    "사용자 TLS 파일이 설정된 요청의 코드 생성은 아직 지원하지 않습니다",
  ],
] as const;

it.each(unsupported)("%s shows a fixed explanation and no code or enabled Copy", async (_name, make, message) => {
  const user = userEvent.setup();
  const copy = vi.spyOn(navigator.clipboard, "writeText");
  const fileRead = vi.spyOn(FileReader.prototype, "readAsArrayBuffer").mockImplementation(() => {
    throw new Error("unexpected file read");
  });
  const fetch = vi.spyOn(globalThis, "fetch").mockRejectedValue(new Error("unexpected file fetch"));
  render(view(make()));
  for (const target of CODE_TARGETS) {
    await user.selectOptions(screen.getByRole("combobox", { name: "언어" }), target);
    expect(screen.getByRole("alert")).toHaveTextContent(message);
    expect(screen.queryByRole("textbox", { name: "생성한 코드" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "복사" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "복사" }));
  }
  expect(document.body).not.toHaveTextContent(privatePath);
  expect(document.body).not.toHaveTextContent("AP+A");
  expect(copy).not.toHaveBeenCalled();
  expect(fileRead).not.toHaveBeenCalled();
  expect(invoke).not.toHaveBeenCalled();
  expect(fetch).not.toHaveBeenCalled();
});

it("unsupported transitions remove prior output and recover when the active body becomes supported", async () => {
  const user = userEvent.setup();
  const copy = vi.spyOn(navigator.clipboard, "writeText");
  const before = updateBody(setBodyKind(base(), "json"), "json", { text: '{"value":"owned-before"}' });
  const rendered = render(view(before));
  expect(screen.getByRole("textbox", { name: "생성한 코드" })).toHaveTextContent("owned-before");
  await user.click(screen.getByRole("button", { name: "복사" }));
  expect(copy).toHaveBeenCalledOnce();
  rendered.rerender(view(encoded()));
  expect(screen.getByRole("alert")).toHaveTextContent("Base64 원시 본문");
  expect(screen.queryByRole("textbox", { name: "생성한 코드" })).not.toBeInTheDocument();
  expect(document.body).not.toHaveTextContent("owned-before");
  expect(screen.queryByText("복사했습니다")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "복사" })).toBeDisabled();
  const after = updateBody(setBodyKind(encoded(), "json"), "json", { text: '{"value":"owned-after"}' });
  rendered.rerender(view(after));
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  expect(screen.getByRole("textbox", { name: "생성한 코드" })).toHaveTextContent("owned-after");
  expect(screen.getByRole("button", { name: "복사" })).toBeEnabled();
  expect(screen.queryByText("복사했습니다")).not.toBeInTheDocument();
  expect(copy).toHaveBeenCalledOnce();
});

it("clearing custom TLS files restores code generation while keeping verify=false behavior", () => {
  const draft = { ...base(), tls: { ...base().tls, verify: false, caPath: privatePath } };
  const rendered = render(view(draft));
  expect(screen.getByRole("alert")).toHaveTextContent("사용자 TLS 파일");
  rendered.rerender(view({ ...draft, tls: { verify: false, caPath: "", clientCertPath: "", clientKeyPath: null } }));
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  expect(screen.getByRole("textbox", { name: "생성한 코드" })).toHaveTextContent("--insecure");
  expect(screen.getByRole("button", { name: "복사" })).toBeEnabled();
});
