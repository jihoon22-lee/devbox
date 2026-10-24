import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import tokenCases from "../../../../../crates/api/tests/fixtures/raw-storage-token-cases.json";
import type { ResponseRule, WebhookReceivedDetail } from "../../../rpc/gen/rpc";
import { activeBody } from "../api/model/draft";
import { fileFromDraft } from "../api/model/fileDraft";
import { serializeRequestFile } from "../api/model/requestFile";
import { RuleEditor } from "./RuleEditor";
import { toRequest } from "./toRequest";

function received(payload: string, contentType: string): WebhookReceivedDetail {
  return {
    received: {
      id: "r",
      listenerId: "l",
      method: "POST",
      path: "/hook",
      atMs: 1,
      sizeBytes: payload.length,
      truncated: false,
    },
    query: "",
    headers: [["Content-Type", contentType]],
    bodyBase64: btoa(Array.from(new TextEncoder().encode(payload), (b) => String.fromCharCode(b)).join("")),
    previewTruncated: false,
  };
}
it.each(["application/json", "Application/JSON; charset=utf-8", "application/problem+json"])(
  "received JSON secrets are redacted before project persistence (%s)",
  (contentType) => {
    const payload = '{"client_secret":"fixture-private-value","password":"fixture-password"}';
    const draft = toRequest(received(payload, contentType), "http://127.0.0.1:1234");
    // Opening the draft must not change what an explicit replay would send.
    expect(activeBody(draft)).toMatchObject({ text: payload });
    const saved = serializeRequestFile(fileFromDraft(draft, "Received"), { kind: "project", projectId: "p" });
    expect(saved.text).not.toContain("fixture-private-value");
    expect(saved.text).not.toContain("fixture-password");
    expect(saved.masked).toContain("request.body.text");
  },
);
it("received form secrets and percent-encoded keys are sanitized without changing the replay draft", () => {
  const payload =
    "safe=a%20b&client_secret=fixture-private-value&%70assword=fixture-password&password=another-password&token=%7B%7Bsecret%3ATOKEN%7D%7D";
  const draft = toRequest(
    received(payload, "application/x-www-form-urlencoded; charset=UTF-8"),
    "http://127.0.0.1:1234",
  );
  expect(activeBody(draft)).toMatchObject({ text: payload });
  const saved = serializeRequestFile(fileFromDraft(draft, "Received"), { kind: "project", projectId: "p" });
  expect(saved.text).not.toContain("fixture-private-value");
  expect(saved.text).not.toContain("fixture-password");
  expect(saved.text).not.toContain("another-password");
  expect(saved.text).toContain("safe=a%20b");
  expect(saved.text).toContain("token=%7B%7Bsecret%3ATOKEN%7D%7D");
  expect(saved.masked).toContain("request.body.text");
});
it("saving a newly created rule again updates its assigned ID", async () => {
  const submitted = vi.fn(async (rule: ResponseRule) => ({ ...rule, id: rule.id || "assigned-id" }));
  render(<RuleEditor items={[]} onDelete={() => {}} onSave={submitted} />);
  fireEvent.click(screen.getByRole("button", { name: "규칙 저장" }));
  await waitFor(() => expect(screen.getByRole("button", { name: "규칙 저장" })).not.toBeDisabled());
  fireEvent.change(screen.getByLabelText("응답 본문"), { target: { value: "edited" } });
  fireEvent.click(screen.getByRole("button", { name: "규칙 저장" }));
  expect(submitted).toHaveBeenCalledTimes(2);
  expect(submitted.mock.calls[1]?.[0].id).toBe("assigned-id");
});
it("an in-flight save adopts identity but keeps newer edits and blocks duplicate submission", async () => {
  let finish: (() => void) | undefined;
  const submitted = vi.fn(
    (rule: ResponseRule) =>
      new Promise<ResponseRule>((resolve) => {
        finish = () => resolve({ ...rule, id: "assigned-id" });
      }),
  );
  render(<RuleEditor items={[]} onDelete={() => {}} onSave={submitted} />);
  fireEvent.click(screen.getByRole("button", { name: "규칙 저장" }));
  fireEvent.change(screen.getByLabelText("응답 본문"), { target: { value: "newer edit" } });
  fireEvent.click(screen.getByRole("button", { name: "규칙 저장" }));
  expect(submitted).toHaveBeenCalledTimes(1);
  await act(async () => {
    finish?.();
  });
  expect(screen.getByLabelText("응답 본문")).toHaveValue("newer edit");
  fireEvent.click(screen.getByRole("button", { name: "규칙 저장" }));
  expect(submitted.mock.calls[1]?.[0]).toMatchObject({ id: "assigned-id", body: "newer edit" });
});

it("a late save does not assign the old ID to a different new draft", async () => {
  let finish: (() => void) | undefined;
  const submitted = vi.fn(
    (rule: ResponseRule) =>
      new Promise<ResponseRule>((resolve) => {
        finish = () => resolve({ ...rule, id: "old-id" });
      }),
  );
  render(<RuleEditor items={[]} onDelete={() => {}} onSave={submitted} />);
  fireEvent.click(screen.getByRole("button", { name: "규칙 저장" }));
  fireEvent.click(screen.getByRole("button", { name: "새 규칙" }));
  fireEvent.change(screen.getByLabelText("경로"), { target: { value: "/new" } });
  await act(async () => {
    finish?.();
  });
  fireEvent.click(screen.getByRole("button", { name: "규칙 저장" }));
  expect(submitted.mock.calls[1]?.[0]).toMatchObject({ id: "", path: "/new" });
});
it("a failed save permits retry without dropping the current draft", async () => {
  const submitted = vi
    .fn()
    .mockRejectedValueOnce(new Error("offline"))
    .mockImplementationOnce(async (rule: ResponseRule) => ({ ...rule, id: "saved" }));
  render(<RuleEditor items={[]} onDelete={() => {}} onSave={submitted} />);
  fireEvent.change(screen.getByLabelText("응답 본문"), { target: { value: "keep me" } });
  fireEvent.click(screen.getByRole("button", { name: "규칙 저장" }));
  await screen.findByRole("alert");
  fireEvent.click(screen.getByRole("button", { name: "규칙 저장" }));
  expect(submitted.mock.calls[1]?.[0]).toMatchObject({ id: "", body: "keep me" });
  await waitFor(() => expect(screen.getByRole("button", { name: "규칙 저장" })).not.toBeDisabled());
});

it.each(["text/plain", "application/json"])(
  "wrapped known token is masked by the real storage serializer (%s)",
  (mime) => {
    const token = "ghp_abcdefghijklmnopqrstuvwxyz1234567890";
    const payload = mime === "application/json" ? JSON.stringify({ message: `(${token})` }) : `(${token})`;
    const draft = toRequest(received(payload, mime), "http://127.0.0.1:1234");
    const saved = serializeRequestFile(fileFromDraft(draft, "Received"), { kind: "project", projectId: "p" });
    expect(saved.text).not.toContain(token);
    expect(saved.text).toContain("[REDACTED]");
    expect(saved.masked).toContain("request.body.text");
  },
);

it("shares stored-token decisions with the daemon corpus, including wrapping and safe references", async () => {
  const cases = tokenCases;
  for (const sample of cases) {
    for (const mode of ["text", "json", "base64"] as const) {
      const payload = mode === "json" ? JSON.stringify({ message: sample.text }) : sample.text;
      const draft = toRequest(
        received(payload, mode === "json" ? "application/json" : "text/plain"),
        "http://localhost",
      );
      if (mode === "base64")
        draft.bodies.raw = {
          text: btoa(Array.from(new TextEncoder().encode(payload), (b) => String.fromCharCode(b)).join("")),
          contentType: "application/octet-stream",
          encoding: "base64",
        };
      const saved = serializeRequestFile(fileFromDraft(draft, "Received"), { kind: "project", projectId: "p" });
      expect(saved.masked.length > 0, `${sample.label} ${mode}`).toBe(sample.sensitive);
      if (!sample.sensitive) expect(saved.masked).toEqual([]);
    }
  }
});

it("large ordinary raw bodies finish sanitization without changing bytes", () => {
  const payload = "a".repeat(200_000);
  const draft = toRequest(received(payload, "text/plain"), "http://localhost");
  const saved = serializeRequestFile(fileFromDraft(draft, "Large"), { kind: "project", projectId: "p" });
  expect(saved.masked).toEqual([]);
  expect(JSON.parse(saved.text).request.body.text).toBe(payload);
});
