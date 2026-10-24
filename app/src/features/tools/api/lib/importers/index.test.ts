import { expect, it } from "vitest";
import insomnia from "./fixtures/insomnia-v4.json";
import postman from "./fixtures/postman-v21.json";
import { detectFormat, ImportError, parseImport } from "./index";

// 적응: v0.9.0 Devbox 내보내기 문서는 v1에서 가져오지 않는다(CONVENTIONS §9) → 감지 결과 null.
it("detects only recognized formats", () => {
  expect(detectFormat("x.har", '{"log":{"entries":[]}}')).toBe("har");
  expect(detectFormat("x.json", JSON.stringify(postman))).toBe("postman");
  expect(detectFormat("x.json", JSON.stringify(insomnia))).toBe("insomnia");
  expect(
    detectFormat(
      "x.json",
      JSON.stringify({ schema: "devbox.api-playground.collection-export", version: 1, collections: [] }),
    ),
  ).toBeNull();
  expect(detectFormat("a.bru", "meta {\n}\n")).toBe("bruno");
  expect(detectFormat("x.json", "{}")).toBeNull();
});
it("projects parse failures to one import error", () => {
  expect(() => parseImport("postman", [{ relativePath: "broken.json", text: "{not json" }])).toThrow(
    new ImportError("가져올 수 없는 형식입니다."),
  );
});
