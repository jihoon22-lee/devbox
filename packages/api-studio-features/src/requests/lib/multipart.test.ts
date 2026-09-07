import { describe, expect, it } from "vitest";
import {
  addMultipartPart,
  duplicateMultipartPart,
  emptyMultipartPart,
  isMultipartDerivedHeader,
  MAX_MULTIPART_PARTS,
  normalizeMultipartParts,
  removeMultipartPart,
  safeMultipartFileName,
  setMultipartFile,
  updateMultipartPart,
  validateMultipartParts,
} from "./multipart";

describe("multipart part model", () => {
  it("kind별 불필요한 값을 제거하고 enabled 누락을 true로 정규화한다", () => {
    expect(normalizeMultipartParts([{
      kind: "text",
      name: "note",
      value: "hello",
      file_path: "C:\\secret.txt",
      file_name: "secret.txt",
      content_type: "text/plain",
    }])).toEqual([{
      kind: "text",
      name: "note",
      value: "hello",
      file_path: "",
      file_name: "",
      content_type: "text/plain",
      enabled: true,
    }]);
  });

  it("kind 전환, 파일 선택, 복제와 삭제가 원본을 변경하지 않는다", () => {
    const source = [emptyMultipartPart()];
    const file = updateMultipartPart(source, 0, { kind: "file" });
    expect(file[0]).toMatchObject({ kind: "file", name: "", file_path: "" });
    const selected = setMultipartFile(file, 0, { path: "C:\\tmp\\a.txt", name: "a.txt" });
    expect(selected[0]).toMatchObject({ file_path: "C:\\tmp\\a.txt", file_name: "a.txt" });
    expect(duplicateMultipartPart(selected, 0)).toHaveLength(2);
    expect(removeMultipartPart(selected, 0)).toEqual([]);
    expect(source).toEqual([emptyMultipartPart()]);
  });

  it("50개 상한을 적용하고 초과 입력은 명시적으로 거부한다", () => {
    const full = Array.from({ length: MAX_MULTIPART_PARTS }, () => emptyMultipartPart());
    expect(addMultipartPart(full)).toHaveLength(MAX_MULTIPART_PARTS);
    expect(validateMultipartParts([...full, { ...emptyMultipartPart(), name: "overflow" }]))
      .toContainEqual({
        index: MAX_MULTIPART_PARTS,
        field: "parts",
        message: "multipart는 최대 50개 part까지 사용할 수 있습니다.",
      });
  });

  it("이름, content type, file 재선택과 text byte 상한을 검증한다", () => {
    const issues = validateMultipartParts([
      { ...emptyMultipartPart(), name: "bad name", value: "x" },
      { ...emptyMultipartPart("file"), name: "upload", file_name: "old.bin" },
      { ...emptyMultipartPart(), name: "meta", value: "x", content_type: "bad type" },
      { ...emptyMultipartPart(), name: "large", value: "가".repeat(400_000) },
    ]);
    expect(issues).toContainEqual({
      index: 0,
      field: "name",
      message: "part 이름은 120자 이하의 HTTP token이어야 합니다.",
    });
    expect(issues).toContainEqual({
      index: 1,
      field: "file",
      message: "'old.bin' 파일을 다시 선택하세요.",
    });
    expect(issues).toContainEqual({
      index: 2,
      field: "content_type",
      message: "Content-Type은 type/subtype 형식이어야 합니다.",
    });
    expect(issues.some((issue) => issue.message.includes("1,000,000바이트"))).toBe(true);
  });

  it("Windows와 POSIX 경로에서 제어 문자를 제거한 basename만 만든다", () => {
    expect(safeMultipartFileName("C:\\work\\report.json")).toBe("report.json");
    expect(safeMultipartFileName("/tmp/bad\nname.txt")).toBe("badname.txt");
  });

  it("multipart boundary와 길이를 만드는 header를 식별한다", () => {
    expect(isMultipartDerivedHeader("Content-Type")).toBe(true);
    expect(isMultipartDerivedHeader("content_length")).toBe(true);
    expect(isMultipartDerivedHeader("Transfer-Encoding")).toBe(true);
    expect(isMultipartDerivedHeader("Content-Encoding")).toBe(false);
  });
});
