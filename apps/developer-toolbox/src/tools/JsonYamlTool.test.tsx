import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { JsonYamlTool } from "./JsonYamlTool";

const writeTextMock = vi.fn<(value: string) => Promise<void>>();
const createObjectUrlMock = vi.fn<(blob: Blob) => string>();
const revokeObjectUrlMock = vi.fn<(url: string) => void>();
let clickedDownload = "";

beforeEach(() => {
  writeTextMock.mockReset().mockResolvedValue(undefined);
  createObjectUrlMock.mockReset().mockReturnValue("blob:json-yaml-result");
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

function input(): HTMLTextAreaElement {
  return screen.getByRole("textbox", { name: /입력$/u }) as HTMLTextAreaElement;
}

describe("JsonYamlTool", () => {
  it("JSON을 YAML로 변환하고 눈에 보이는 복사·저장 동작을 제공한다", async () => {
    render(<JsonYamlTool />);
    fireEvent.change(input(), { target: { value: '{"name":"devbox"}' } });

    const output = screen.getByLabelText("JSON → YAML 출력");
    expect(output.textContent).toBe("name: devbox\n");

    fireEvent.click(screen.getByRole("button", { name: "복사" }));
    await waitFor(() => expect(writeTextMock).toHaveBeenCalledWith("name: devbox\n"));

    fireEvent.click(screen.getByRole("button", { name: "저장" }));
    expect(createObjectUrlMock).toHaveBeenCalledTimes(1);
    expect(clickedDownload).toBe("converted.yaml");
    expect(revokeObjectUrlMock).toHaveBeenCalledWith("blob:json-yaml-result");
  });

  it("YAML → JSON 전환 시 손실 안내를 계속 표시하고 JSON 확장 결과를 만든다", () => {
    render(<JsonYamlTool />);
    fireEvent.click(screen.getByRole("button", { name: "YAML → JSON" }));

    expect(screen.getByRole("note").textContent).toContain("주석은 제거");
    expect(screen.getByRole("note").textContent).toContain("anchor 이름과 공유 관계는 보존되지 않습니다");

    fireEvent.change(input(), {
      target: { value: "base: &base\n  retries: 3\ncopy: *base" },
    });
    expect(JSON.parse(screen.getByLabelText("YAML → JSON 출력").textContent ?? "")).toEqual({
      base: { retries: 3 },
      copy: { retries: 3 },
    });
    fireEvent.click(screen.getByRole("button", { name: "저장" }));
    expect(clickedDownload).toBe("converted.json");
  });

  it("parse 오류의 위치를 보여 주고 결과 action을 비활성화한다", () => {
    render(<JsonYamlTool />);
    fireEvent.change(input(), { target: { value: '{\n  "broken": }' } });

    const alert = screen.getByRole("alert");
    expect(alert.textContent).toContain("2행");
    expect(alert.textContent).toContain("INVALID_JSON");
    expect((screen.getByRole("button", { name: "복사" }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole("button", { name: "저장" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("현재 결과를 반대 방향 입력으로 넘겨 왕복 변환한다", () => {
    render(<JsonYamlTool />);
    fireEvent.change(input(), { target: { value: '{"name":"devbox"}' } });
    fireEvent.click(screen.getByRole("button", { name: "결과를 반대 방향 입력으로 사용" }));

    expect(screen.getByRole("button", { name: "YAML → JSON" }).getAttribute("aria-pressed")).toBe("true");
    expect(input().value).toBe("name: devbox\n");
    expect(JSON.parse(screen.getByLabelText("YAML → JSON 출력").textContent ?? "")).toEqual({
      name: "devbox",
    });
  });
});
