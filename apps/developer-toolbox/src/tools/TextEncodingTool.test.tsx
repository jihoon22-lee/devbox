import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { TransformerTool } from "./common";
import { HtmlEntityDecoder, HtmlEntityEncoder, UrlDecoder } from "./transformers";

afterEach(() => cleanup());

describe("HTML entity and URL component tool surfaces", () => {
  it("renders the HTML entity encoder result with accessible input/output names", async () => {
    render(<HtmlEntityEncoder />);
    const input = screen.getByRole("textbox", { name: "입력" });
    expect(screen.getByLabelText("출력")).toBeTruthy();

    fireEvent.change(input, { target: { value: `<hello & "세계">` } });
    await waitFor(() =>
      expect(screen.getByLabelText("출력").textContent).toBe(
        "&lt;hello &amp; &quot;세계&quot;&gt;",
      ),
    );
  });

  it("shows a fixed decoder error without echoing malformed input", async () => {
    render(<HtmlEntityDecoder />);
    fireEvent.change(screen.getByRole("textbox", { name: "입력" }), {
      target: { value: "&credential=super-secret;" },
    });

    await waitFor(() =>
      expect(screen.getByLabelText("출력").textContent).toBe(
        "HTML entity contains malformed or unsupported syntax.",
      ),
    );
    expect(screen.getByLabelText("출력").textContent).not.toContain("super-secret");
  });

  it("keeps invalid URL input in a fixed, offline error state", async () => {
    render(<UrlDecoder />);
    fireEvent.change(screen.getByRole("textbox", { name: "입력" }), {
      target: { value: "%zz?token=super-secret" },
    });

    await waitFor(() =>
      expect(screen.getByLabelText("출력").textContent).toBe(
        "URL component contains malformed percent-encoding.",
      ),
    );
    expect(screen.getByLabelText("출력").textContent).not.toContain("super-secret");
  });
});

describe("bounded transform stale/busy state", () => {
  const pending: Array<{
    input: string;
    resolve: (result: { output: string }) => void;
  }> = [];

  const deferredRun = (input: string) =>
    new Promise<{ output: string }>((resolve) => {
      pending.push({ input, resolve });
    });

  beforeEach(() => {
    pending.length = 0;
  });

  it("clears the previous output and ignores an older completion", async () => {
    render(
      <TransformerTool
        placeholder="Text"
        run={deferredRun}
        clearOutputOnInput
      />,
    );
    const input = screen.getByRole("textbox", { name: "입력" });
    const output = screen.getByLabelText("출력");

    fireEvent.change(input, { target: { value: "seed" } });
    const seed = pending.find((entry) => entry.input === "seed");
    expect(seed).toBeDefined();
    seed?.resolve({ output: "seed result" });
    await waitFor(() => expect(output.textContent).toBe("seed result"));

    fireEvent.change(input, { target: { value: "first" } });
    expect(output.textContent).toBe(" ");
    fireEvent.change(input, { target: { value: "second" } });
    expect(input.getAttribute("aria-busy")).toBe("true");
    expect(screen.getByRole("status").textContent).toBe("(실행 중...)");

    const second = pending.find((entry) => entry.input === "second");
    const first = pending.find((entry) => entry.input === "first");
    expect(second).toBeDefined();
    expect(first).toBeDefined();
    second?.resolve({ output: "second result" });
    await waitFor(() => expect(output.textContent).toBe("second result"));

    first?.resolve({ output: "stale first result" });
    await waitFor(() => expect(output.textContent).toBe("second result"));
    expect(output.textContent).not.toContain("stale");
  });

  it("invalidates a pending completion when the tool unmounts", async () => {
    const { unmount } = render(
      <TransformerTool
        placeholder="Text"
        run={deferredRun}
        clearOutputOnInput
      />,
    );
    fireEvent.change(screen.getByRole("textbox", { name: "입력" }), {
      target: { value: "pending" },
    });
    const request = pending.find((entry) => entry.input === "pending");
    expect(request).toBeDefined();

    unmount();
    request?.resolve({ output: "must be ignored" });
    await Promise.resolve();
  });
});
