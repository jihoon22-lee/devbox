import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MultipartEditor } from "./MultipartEditor";
import { emptyMultipartPart, type PickedMultipartFile } from "./lib/multipart";
import type { MultipartPart } from "./types";

afterEach(cleanup);

function setup(
  rows: MultipartPart[],
  onPickFile: () => Promise<PickedMultipartFile | null> = vi.fn(async () => null),
  secretNames: string[] = [],
) {
  const onChange = vi.fn<(rows: MultipartPart[]) => void>();
  render(
    <MultipartEditor
      rows={rows}
      secretNames={secretNames}
      onChange={onChange}
      onPickFile={onPickFile}
    />,
  );
  return { onChange };
}

describe("MultipartEditor", () => {
  it("file picker의 전체 경로를 화면에 노출하지 않고 runtime 모델에만 전달한다", async () => {
    const picker = vi.fn(async () => ({
      path: "C:\\private\\build\\artifact.zip",
      name: "C:\\private\\build\\artifact.zip",
    }));
    const { onChange } = setup([{ ...emptyMultipartPart("file"), name: "upload" }], picker);

    fireEvent.click(screen.getByRole("button", { name: "1번 파일 선택" }));
    await waitFor(() => expect(onChange).toHaveBeenCalled());
    expect(onChange.mock.calls[0][0][0]).toMatchObject({
      file_path: "C:\\private\\build\\artifact.zip",
      file_name: "artifact.zip",
    });
    expect(document.body.textContent).not.toContain("C:\\private");
  });

  it("text part에 봉인 secret의 이름 참조만 삽입한다", () => {
    const { onChange } = setup(
      [{ ...emptyMultipartPart(), name: "token" }],
      undefined,
      ["TOKEN", "bad name"],
    );
    const select = screen.getByLabelText("1번 part secret 참조") as HTMLSelectElement;
    expect([...select.options].map((option) => option.text)).toEqual(["Secret 참조", "TOKEN"]);
    fireEvent.change(select, { target: { value: "TOKEN" } });
    expect(onChange).toHaveBeenCalledWith([
      { ...emptyMultipartPart(), name: "token", value: "${TOKEN}" },
    ]);
  });

  it("저장 후 경로가 제거된 file part에 재선택 오류를 표시한다", () => {
    setup([{ ...emptyMultipartPart("file"), name: "upload", file_name: "artifact.zip" }]);
    expect(screen.getByRole("alert").textContent).toContain("'artifact.zip' 파일을 다시 선택하세요.");
  });

  it("picker 실패를 안전한 메시지로 표시한다", async () => {
    const picker = vi.fn(async () => { throw new Error("C:\\private\\secret.txt"); });
    setup([{ ...emptyMultipartPart("file"), name: "upload" }], picker);
    fireEvent.click(screen.getByRole("button", { name: "1번 파일 선택" }));
    await waitFor(() => expect(screen.getByText(/데스크톱 앱 권한을 확인하세요/u)).toBeTruthy());
    expect(document.body.textContent).not.toContain("secret.txt");
  });
});
