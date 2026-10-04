import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { RequestCodePanels } from "./RequestCodePanels";
import { confirmAction } from "@devbox/product-shell/confirm";
import { buildRevealedCurl } from "../api";
import type { RequestTemplate } from "../types";
vi.mock("@devbox/product-shell/confirm", () => ({ confirmAction: vi.fn() }));
vi.mock("../api", () => ({ buildRevealedCurl: vi.fn() }));
const request: RequestTemplate = {
  method: "GET",
  url: "https://synthetic.test",
  headers: [],
  params: [],
  cookies: [],
  multipart: [],
  body_kind: "none",
  body: "",
  auth: null,
  timeout_ms: 30000,
};
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});
it.each(["unmount", "request change"])(
  "never reveals stale secret after pending confirmation and %s",
  async (change) => {
    let decide!: (accepted: boolean) => void;
    vi.mocked(confirmAction).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          decide = resolve;
        }),
    );
    const writeText = vi.fn();
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    const props = {
      request,
      environment: [],
      codeEnvironment: [],
      showCurl: true,
      showCode: false,
      configurationError: null,
      onError: vi.fn(),
    };
    const view = render(<RequestCodePanels {...props} />);
    fireEvent.click(screen.getByRole("button", { name: "원문 1회 복사" }));
    if (change === "unmount") view.unmount();
    else
      view.rerender(<RequestCodePanels {...props} request={{ ...request, url: "https://synthetic.test/changed" }} />);
    await act(async () => {
      decide(true);
    });
    expect(buildRevealedCurl).not.toHaveBeenCalled();
    expect(writeText).not.toHaveBeenCalled();
  },
);
it("does not copy when owner disappears during native secret retrieval", async () => {
  vi.mocked(confirmAction).mockResolvedValueOnce(true);
  let reveal!: (value: string) => void;
  vi.mocked(buildRevealedCurl).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        reveal = resolve;
      }),
  );
  const writeText = vi.fn();
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
  const view = render(
    <RequestCodePanels
      request={request}
      environment={[]}
      codeEnvironment={[]}
      showCurl
      showCode={false}
      configurationError={null}
      onError={vi.fn()}
    />,
  );
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "원문 1회 복사" }));
  });
  view.unmount();
  await act(async () => {
    reveal("synthetic-secret-curl");
  });
  expect(writeText).not.toHaveBeenCalled();
});
