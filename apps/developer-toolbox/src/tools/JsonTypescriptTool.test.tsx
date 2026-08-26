import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { JsonTypescriptTool } from "./JsonTypescriptTool";

const writeTextMock = vi.fn<(value: string) => Promise<void>>();
const createObjectUrlMock = vi.fn<(blob: Blob) => string>();
const revokeObjectUrlMock = vi.fn<(url: string) => void>();
let clickedDownload = "";

beforeEach(() => {
  writeTextMock.mockReset().mockResolvedValue(undefined);
  createObjectUrlMock.mockReset().mockReturnValue("blob:json-typescript-result");
  revokeObjectUrlMock.mockReset();
  clickedDownload = "";
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: writeTextMock },
  });
  Object.defineProperty(URL, "createObjectURL", {
    configurable: true,
    value: createObjectUrlMock,
  });
  Object.defineProperty(URL, "revokeObjectURL", {
    configurable: true,
    value: revokeObjectUrlMock,
  });
  vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (this: HTMLAnchorElement) {
    clickedDownload = this.download;
  });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("JsonTypescriptTool", () => {
  it("root type 이름과 JSON 표본으로 optional TypeScript 결과를 만든다", () => {
    render(<JsonTypescriptTool />);
    fireEvent.change(screen.getByRole("textbox", { name: "Root type 이름" }), {
      target: { value: "ApiResponse" },
    });
    fireEvent.change(screen.getByRole("textbox", { name: "JSON → TypeScript 입력" }), {
      target: { value: '{"users":[{"id":1,"name":"Ada"},{"id":2}]}' },
    });

    const output = screen.getByLabelText("JSON → TypeScript 출력");
    expect(output.textContent).toContain("export interface ApiResponse");
    expect(output.textContent).toContain("name?: string;");
    expect(screen.getByRole("note").textContent).toContain("빈 배열의 원소는 unknown");
    expect(screen.getByRole("note").textContent).toContain("자동 저장하거나 외부로 전송하지 않습니다");
  });

  it("결과를 clipboard와 root 이름 기반 .ts 파일로 내보낸다", async () => {
    render(<JsonTypescriptTool />);
    fireEvent.change(screen.getByRole("textbox", { name: "Root type 이름" }), {
      target: { value: "Project" },
    });
    fireEvent.change(screen.getByRole("textbox", { name: "JSON → TypeScript 입력" }), {
      target: { value: '{"name":"devbox"}' },
    });

    const expected = "export interface Project {\n  name: string;\n}\n";
    fireEvent.click(screen.getByRole("button", { name: "결과 복사" }));
    await waitFor(() => expect(writeTextMock).toHaveBeenCalledWith(expected));

    fireEvent.click(screen.getByRole("button", { name: ".ts 저장" }));
    expect(clickedDownload).toBe("Project.ts");
    expect(revokeObjectUrlMock).toHaveBeenCalledWith("blob:json-typescript-result");
  });

  it("parse 위치 오류와 root 이름 오류에서 결과 action을 비활성화한다", () => {
    render(<JsonTypescriptTool />);
    const jsonInput = screen.getByRole("textbox", { name: "JSON → TypeScript 입력" });
    fireEvent.change(jsonInput, { target: { value: '{\n  "broken": }' } });

    expect(screen.getByRole("alert").textContent).toContain("2행");
    expect(screen.getByRole("alert").textContent).toContain("INVALID_JSON");
    expect((screen.getByRole("button", { name: "결과 복사" }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole("button", { name: ".ts 저장" }) as HTMLButtonElement).disabled).toBe(true);

    fireEvent.change(screen.getByRole("textbox", { name: "Root type 이름" }), {
      target: { value: "invalid name" },
    });
    expect(screen.getByRole("alert").textContent).toContain("INVALID_ROOT_TYPE_NAME");
  });

  it("clipboard 실패 상세를 반향하지 않고 생성 결과를 유지한다", async () => {
    writeTextMock.mockRejectedValueOnce(new Error("DO_NOT_REFLECT_CLIPBOARD_SECRET"));
    render(<JsonTypescriptTool />);
    fireEvent.change(screen.getByRole("textbox", { name: "JSON → TypeScript 입력" }), {
      target: { value: '{"name":"devbox"}' },
    });

    fireEvent.click(screen.getByRole("button", { name: "결과 복사" }));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe("TypeScript 결과를 clipboard에 복사하지 못했습니다.");
    expect(alert.textContent).not.toContain("DO_NOT_REFLECT_CLIPBOARD_SECRET");
    expect(screen.getByLabelText("JSON → TypeScript 출력").textContent).toContain("name: string;");
  });
});
