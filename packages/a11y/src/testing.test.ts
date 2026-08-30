import { describe, expect, it } from "vitest";
import { assertNoA11yViolations, findA11yViolations } from "./testing";

describe("axe helper", () => {
  it("accepts a labelled application shell", async () => {
    document.body.innerHTML = `
      <main aria-labelledby="title">
        <h1 id="title">Devbox</h1>
        <label for="query">검색</label><input id="query" />
        <button type="button">실행</button>
      </main>`;
    await expect(assertNoA11yViolations(document.querySelector("main")!)).resolves.toBeUndefined();
  });

  it("reports structural violations with useful identifiers", async () => {
    document.body.innerHTML = `<main><input /></main>`;
    const violations = await findA11yViolations(document.querySelector("main")!);
    expect(violations.map((violation) => violation.id)).toContain("label");
    await expect(assertNoA11yViolations(document.querySelector("main")!)).rejects.toThrow(/label/);
  });
});
