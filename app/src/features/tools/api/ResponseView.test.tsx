import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { emptyDraft } from "./model/draft";
import { ResponseView } from "./ResponseView";
import { originOf } from "./sendState";
import { textResponse } from "./test-backend";

afterEach(cleanup);

// AR-F08(HTTP): 응답 머리에 만든 요청(이름·메서드·URL·보낸 시각), 요청을 고치면 "편집 중인 요청과 다름".
it("response_origin_marks_edited_request", () => {
  const draft = { ...emptyDraft(), method: "POST", url: "https://x.test/a" };
  const response = textResponse('{"ok":true}', 201, { reason: "Created", sentAtMs: Date.UTC(2026, 9, 22, 1, 2, 3) });
  const result = { response, origin: originOf("Login", draft, response.sentAtMs) };
  const { rerender } = render(<ResponseView result={result} draft={draft} />);
  const origin = screen.getByRole("group", { name: "응답을 만든 요청" });
  expect(origin).toHaveTextContent("Login");
  expect(origin).toHaveTextContent("POST");
  expect(origin).toHaveTextContent("https://x.test/a");
  expect(screen.getByRole("time")).toHaveAttribute("datetime", "2026-10-22T01:02:03.000Z");
  expect(screen.queryByText("편집 중인 요청과 다름")).toBeNull();
  expect(screen.getByText("201 Created")).toBeInTheDocument();
  expect(screen.getByText(/"ok": true/)).toBeInTheDocument();
  rerender(<ResponseView result={result} draft={{ ...draft, url: "https://x.test/b" }} />);
  expect(screen.getByText("편집 중인 요청과 다름")).toBeInTheDocument();
  // 다시 같은 요청으로 돌리면 표시가 사라진다.
  rerender(<ResponseView result={result} draft={{ ...draft }} />);
  expect(screen.queryByText("편집 중인 요청과 다름")).toBeNull();
});

it("empty_and_failed_responses_are_explained", () => {
  const { rerender } = render(<ResponseView draft={emptyDraft()} />);
  expect(screen.getByText("요청을 보내면 응답이 여기에 표시됩니다")).toBeInTheDocument();
  rerender(<ResponseView draft={emptyDraft()} failure={{ code: "api.network", detail: { kind: "connect" } }} />);
  expect(screen.getByRole("alert")).toHaveTextContent("요청 전송에 실패했습니다.");
});
