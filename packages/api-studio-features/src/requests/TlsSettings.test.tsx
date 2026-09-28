import { useState } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { TlsSettings } from "./TlsSettings";
import { listGrpcTlsCredentials } from "./grpcApi";
import type { RequestTls } from "../generated/RequestTls";
vi.mock("./grpcApi", () => ({ listGrpcTlsCredentials: vi.fn() }));
vi.mock("./lib/isTauri", () => ({ isTauri: () => true }));
afterEach(cleanup);
beforeEach(() =>
  vi
    .mocked(listGrpcTlsCredentials)
    .mockReset()
    .mockResolvedValue([
      { credentialId: "a".repeat(32), label: "테스트 CA", hasCustomCa: true, hasClientIdentity: false, createdAtMs: 1 },
    ]),
);
it("selects shared TLS credentials, turns verification off and remains accessible", async () => {
  function Harness() {
    const [value, onChange] = useState<RequestTls>({ credentialId: null, verify: true });
    return <TlsSettings value={value} onChange={onChange} />;
  }
  const { container } = render(<Harness />);
  await screen.findByRole("option", { name: "테스트 CA" });
  fireEvent.change(screen.getByLabelText("TLS 자격 증명"), { target: { value: "a".repeat(32) } });
  expect((screen.getByLabelText("TLS 자격 증명") as HTMLSelectElement).value).toBe("a".repeat(32));
  fireEvent.click(screen.getByLabelText("인증서 검증"));
  expect(screen.getByText("인증서 검증 꺼짐")).toBeTruthy();
  await assertNoA11yViolations(container);
});
it("keeps a missing credential selected instead of silently using system defaults", async () => {
  const onChange = vi.fn();
  render(<TlsSettings value={{ credentialId: "b".repeat(32), verify: true }} onChange={onChange} />);
  await screen.findByRole("option", { name: "테스트 CA" });
  expect((screen.getByLabelText("TLS 자격 증명") as HTMLSelectElement).value).toBe("b".repeat(32));
  expect(screen.getByRole("option", { name: "찾을 수 없는 자격 증명" })).toBeTruthy();
  expect(onChange).not.toHaveBeenCalled();
});
