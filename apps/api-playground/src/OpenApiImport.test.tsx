import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { fetchOpenApiSource } from "./api";
import { OpenApiImport } from "./OpenApiImport";
import type { OpenApiOperationPreview } from "./lib/openapi";

vi.mock("./api", () => ({ fetchOpenApiSource: vi.fn() }));

const mockedFetchOpenApiSource = vi.mocked(fetchOpenApiSource);

afterEach(() => {
  cleanup();
  mockedFetchOpenApiSource.mockReset();
});

function fixture(paths: Record<string, unknown> = { "/users": { get: {} } }): string {
  return JSON.stringify({
    openapi: "3.0.3",
    info: { title: "fixture", version: "1" },
    servers: [{ url: "https://api.example.test" }],
    paths,
  });
}

function fileWithText(text: string, name = "api.json"): File {
  const file = new File([text], name, { type: "application/json" });
  Object.defineProperty(file, "text", { configurable: true, value: () => Promise.resolve(text) });
  return file;
}

function setup() {
  const onClose = vi.fn<() => void>();
  const onApply = vi.fn();
  const onAddToCollection = vi.fn<(operations: OpenApiOperationPreview[]) => Promise<void>>().mockResolvedValue(undefined);
  const rendered = render(<OpenApiImport onClose={onClose} onApply={onApply} onAddToCollection={onAddToCollection} />);
  return { onClose, onApply, onAddToCollection, rendered };
}

describe("OpenApiImport", () => {
  it("reads only a local file, previews it, and applies only after explicit confirmation", async () => {
    const { onClose, onApply } = setup();
    fireEvent.change(screen.getByLabelText("로컬 파일 선택"), {
      target: { files: [fileWithText(fixture())] },
    });

    await screen.findByText("GET /users");
    expect(onApply).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "현재 초안에 적용" }));
    expect(onApply).toHaveBeenCalledTimes(1);
    expect(onApply.mock.calls[0][0].request.url).toBe("https://api.example.test/users");
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("adds all checked operations as new collection entries without sending", async () => {
    const { onAddToCollection } = setup();
    fireEvent.change(screen.getByLabelText("로컬 파일 선택"), {
      target: {
        files: [fileWithText(fixture({ "/a": { get: {} }, "/b": { post: {} } }))],
      },
    });

    await screen.findByText("GET /a");
    await screen.findByText("POST /b");
    fireEvent.click(screen.getByRole("button", { name: "새 컬렉션에 추가 (2)" }));
    await waitFor(() => expect(onAddToCollection).toHaveBeenCalledTimes(1));
    expect(onAddToCollection.mock.calls[0][0]).toHaveLength(2);
  });

  it("fetches a URL only after submit and parses the bounded native result", async () => {
    mockedFetchOpenApiSource.mockResolvedValue({ text: fixture(), format: "json" });
    setup();
    fireEvent.change(screen.getByLabelText("OpenAPI URL"), {
      target: { value: "https://example.test/openapi.json" },
    });
    expect(mockedFetchOpenApiSource).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "URL 가져오기" }));
    await screen.findByText("GET /users");
    expect(mockedFetchOpenApiSource).toHaveBeenCalledTimes(1);
    expect(mockedFetchOpenApiSource).toHaveBeenCalledWith("https://example.test/openapi.json");
    expect(screen.getByText("remote-openapi.json")).toBeTruthy();
  });

  it("does not reflect a rejected URL and ignores duplicate submits while loading", async () => {
    let rejectFetch: ((reason?: unknown) => void) | undefined;
    mockedFetchOpenApiSource.mockReturnValue(new Promise((_resolve, reject) => {
      rejectFetch = reject;
    }));
    const { onClose } = setup();
    const rawUrl = "https://example.test/DO_NOT_REFLECT/openapi.json";
    fireEvent.change(screen.getByLabelText("OpenAPI URL"), { target: { value: rawUrl } });
    const submit = screen.getByRole("button", { name: "URL 가져오기" });
    fireEvent.click(submit);
    fireEvent.submit(submit.closest("form")!);
    expect(mockedFetchOpenApiSource).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();
    rejectFetch?.(new Error(rawUrl));
    await screen.findByRole("alert");
    expect(screen.getByRole("alert").textContent).toBe("OpenAPI URL을 안전하게 가져오지 못했습니다.");
    expect(document.body.textContent).not.toContain("DO_NOT_REFLECT");
  });

  it("discards a URL result completed after unmount", async () => {
    let resolveFetch: ((source: { text: string; format: "json" }) => void) | undefined;
    mockedFetchOpenApiSource.mockReturnValue(new Promise((resolve) => {
      resolveFetch = resolve;
    }));
    const { onClose, rendered } = setup();
    fireEvent.change(screen.getByLabelText("OpenAPI URL"), {
      target: { value: "https://example.test/openapi.json" },
    });
    fireEvent.click(screen.getByRole("button", { name: "URL 가져오기" }));
    rendered.unmount();

    await act(async () => {
      resolveFetch?.({ text: fixture(), format: "json" });
      await Promise.resolve();
    });
    expect(onClose).not.toHaveBeenCalled();
  });

  it("does not close during IME composition but Escape closes the accessible dialog", () => {
    const { onClose } = setup();
    const dialog = screen.getByRole("dialog");
    fireEvent.keyDown(dialog, { key: "Escape", isComposing: true });
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("keeps keyboard focus inside the modal and restores the opener", () => {
    const opener = document.createElement("button");
    opener.textContent = "OpenAPI opener";
    document.body.appendChild(opener);
    opener.focus();
    setup();
    const dialog = screen.getByRole("dialog");
    const focusable = [...dialog.querySelectorAll<HTMLElement>(
      "button:not([disabled]), input:not([disabled]):not([type=hidden]), select:not([disabled]), textarea:not([disabled]), a[href], [tabindex]:not([tabindex=\"-1\"])",
    )];
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    last.focus();
    fireEvent.keyDown(dialog, { key: "Tab" });
    expect(document.activeElement).toBe(first);
    first.focus();
    fireEvent.keyDown(dialog, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(last);
    cleanup();
    expect(document.activeElement).toBe(opener);
    opener.remove();
  });

  it("renders operation errors as disabled preview rows", async () => {
    setup();
    fireEvent.change(screen.getByLabelText("로컬 파일 선택"), {
      target: {
        files: [fileWithText(fixture({ "/ref": { get: { "$ref": "#/components/path" } } }))],
      },
    });
    const row = await screen.findByText("GET /ref");
    expect(row.closest("label")?.className).toContain("is-invalid");
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
