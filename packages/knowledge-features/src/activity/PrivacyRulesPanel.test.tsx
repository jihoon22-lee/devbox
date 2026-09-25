import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { afterEach, describe, expect, it, vi } from "vitest";
import PrivacyRulesPanel, { fromLines, toLines } from "./PrivacyRulesPanel";
import { EMPTY_PRIVACY_RULES } from "./api";

const api = vi.hoisted(() => ({
  setPrivacyRules: vi.fn(),
  redactExisting: vi.fn(),
}));
vi.mock("./api", async (original) => ({ ...(await original<typeof import("./api")>()), ...api }));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("line helpers", () => {
  it("keeps commas and inner spaces, drops blank lines", () => {
    expect(fromLines("patient \\d{1,3}\n\n  InPrivate - Microsoft Edge  \n")).toEqual([
      "patient \\d{1,3}",
      "InPrivate - Microsoft Edge",
    ]);
    expect(toLines(["a", "b"])).toBe("a\nb");
  });
});

describe("PrivacyRulesPanel", () => {
  it("keeps typed commas and spaces and saves one rule per line", async () => {
    api.setPrivacyRules.mockResolvedValue({ saved: true, invalid: [] });
    const onSaved = vi.fn();
    render(<PrivacyRulesPanel initial={EMPTY_PRIVACY_RULES} healthy onSaved={onSaved} />);
    const redact = screen.getByLabelText("제목 치환 정규식");
    fireEvent.change(redact, { target: { value: "patient \\d{1,3}\nInPrivate - " } });
    expect((redact as HTMLTextAreaElement).value).toBe("patient \\d{1,3}\nInPrivate - ");
    fireEvent.click(screen.getByRole("button", { name: "규칙 저장" }));
    await waitFor(() => expect(onSaved).toHaveBeenCalled());
    expect(api.setPrivacyRules).toHaveBeenCalledWith({
      ...EMPTY_PRIVACY_RULES,
      redactTitlePatterns: ["patient \\d{1,3}", "InPrivate -"],
    });
  });

  it("shows validation problems and does not report success", async () => {
    api.setPrivacyRules.mockResolvedValue({
      saved: false,
      invalid: [{ field: "excludedTitlePatterns", index: 1, problem: "syntax" }],
    });
    const onSaved = vi.fn();
    render(<PrivacyRulesPanel initial={EMPTY_PRIVACY_RULES} healthy onSaved={onSaved} />);
    fireEvent.change(screen.getByLabelText("제목을 저장하지 않을 정규식"), { target: { value: "ok\n(" } });
    fireEvent.click(screen.getByRole("button", { name: "규칙 저장" }));
    expect(await screen.findByText("2번째 규칙: 정규식 문법 오류입니다.")).toBeTruthy();
    expect(onSaved).not.toHaveBeenCalled();
  });

  it("warns when stored rules were unreadable and blocks retroactive apply", () => {
    render(<PrivacyRulesPanel initial={EMPTY_PRIVACY_RULES} healthy={false} onSaved={vi.fn()} />);
    expect(screen.getByRole("alert").textContent).toContain("창 제목 저장을 멈췄습니다");
    expect((screen.getByRole("button", { name: "기존 세션에 적용" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("requires saving before applying rules to stored sessions", () => {
    render(<PrivacyRulesPanel initial={EMPTY_PRIVACY_RULES} healthy onSaved={vi.fn()} />);
    fireEvent.change(screen.getByLabelText("제외할 프로세스"), { target: { value: "LockApp.exe" } });
    expect((screen.getByRole("button", { name: "기존 세션에 적용" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("has no accessibility violations", async () => {
    const { container } = render(<PrivacyRulesPanel initial={EMPTY_PRIVACY_RULES} healthy onSaved={vi.fn()} />);
    await assertNoA11yViolations(container);
  });
});

it("confirms irreversible changes to stored sessions inside the panel", async () => {
  api.redactExisting.mockResolvedValue(2);
  render(<PrivacyRulesPanel initial={EMPTY_PRIVACY_RULES} healthy onSaved={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "기존 세션에 적용" }));
  expect(api.redactExisting).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "기록 변경 확인" }));
  expect(await screen.findByText("기존 세션 2개에 규칙을 적용했습니다.")).toBeTruthy();
});

it("lets unreadable empty rules be saved again", async () => {
  api.setPrivacyRules.mockResolvedValue({ saved: true, invalid: [] });
  const onSaved = vi.fn();
  render(<PrivacyRulesPanel initial={EMPTY_PRIVACY_RULES} healthy={false} onSaved={onSaved} />);
  fireEvent.click(screen.getByRole("button", { name: "규칙 저장" }));
  await waitFor(() => expect(onSaved).toHaveBeenCalledWith(EMPTY_PRIVACY_RULES));
});
