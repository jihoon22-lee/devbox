import { StrictMode, useState } from "react";
import { cleanup, fireEvent, render, screen, waitFor, act } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { OAuth2Editor } from "./OAuth2Editor";
import { oauth2Status, authorizeOAuth2, cancelOAuth2, fetchOAuth2Token, clearOAuth2Token } from "./api";
import type { AuthConfig } from "./types";
vi.mock("./api", () => ({
  oauth2Status: vi.fn(),
  authorizeOAuth2: vi.fn(),
  cancelOAuth2: vi.fn(),
  fetchOAuth2Token: vi.fn(),
  clearOAuth2Token: vi.fn(),
}));
vi.mock("./lib/isTauri", () => ({ isTauri: () => true }));
const config = {
  grantType: "authorizationCode" as const,
  authorizationUrl: "https://auth.test/authorize",
  tokenUrl: "https://auth.test/token",
  clientId: "client",
  clientSecret: "{{SECRET}}",
  scopes: "read",
};
const auth: AuthConfig = {
  kind: "oauth2",
  username: "",
  password: "",
  token: "",
  api_key: "",
  api_value: "",
  oauth2: config,
};
const environment = [{ key: "SECRET", value: "sealed", secret: true }];
const missing = { state: "missing" as const, expiresAtMs: null, scope: null };
function Harness() {
  const [value, setValue] = useState(auth);
  return <OAuth2Editor auth={value} environment={environment} onChange={(oauth2) => setValue({ ...value, oauth2 })} />;
}
afterEach(cleanup);
beforeEach(() => {
  vi.mocked(oauth2Status).mockReset().mockResolvedValue(missing);
  vi.mocked(authorizeOAuth2).mockReset();
  vi.mocked(cancelOAuth2).mockReset().mockResolvedValue(undefined);
  vi.mocked(fetchOAuth2Token).mockReset();
  vi.mocked(clearOAuth2Token).mockReset().mockResolvedValue(undefined);
});
it("shows both grants and sealed token status without any token value", async () => {
  vi.mocked(oauth2Status).mockResolvedValue({ state: "valid", expiresAtMs: Date.now() + 720000, scope: "read" });
  const { container } = render(<Harness />);
  expect(await screen.findByText(/유효 · 12분 뒤 만료/)).toBeTruthy();
  expect(screen.getByLabelText("Authorization URL")).toBeTruthy();
  expect(screen.getByText("환경 변수의 비밀 값({{이름}})을 쓰세요.")).toBeTruthy();
  fireEvent.change(screen.getByLabelText("방식"), { target: { value: "clientCredentials" } });
  expect(screen.queryByLabelText("Authorization URL")).toBeNull();
  expect(screen.getByRole("button", { name: "토큰 받기" })).toBeTruthy();
  await assertNoA11yViolations(container);
});
it("opens login, keeps the operation pending until cancellation settles, and cancels its scoped request", async () => {
  let reject!: (cause: Error) => void;
  vi.mocked(authorizeOAuth2).mockImplementation(
    () =>
      new Promise((_, fail) => {
        reject = fail;
      }),
  );
  vi.mocked(cancelOAuth2).mockImplementation(async () => {
    reject(Object.assign(new Error("private"), { name: "oauth2_cancelled" }));
  });
  render(<Harness />);
  fireEvent.click(screen.getByRole("button", { name: "로그인" }));
  expect(await screen.findByText("브라우저에서 로그인을 마쳐 주세요.")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "취소" }));
  await waitFor(() => expect(cancelOAuth2).toHaveBeenCalledWith(vi.mocked(authorizeOAuth2).mock.calls[0][0]));
  await waitFor(() =>
    expect((screen.getByRole("button", { name: "로그인" }) as HTMLButtonElement).disabled).toBe(false),
  );
  expect(screen.queryByText("private")).toBeNull();
});
it("fetches a client token and clears it", async () => {
  vi.mocked(fetchOAuth2Token).mockResolvedValue({ state: "valid", expiresAtMs: null, scope: "read" });
  render(<Harness />);
  fireEvent.change(screen.getByLabelText("방식"), { target: { value: "clientCredentials" } });
  fireEvent.click(screen.getByRole("button", { name: "토큰 받기" }));
  expect(await screen.findByText("유효 · 만료 시각 없음")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "토큰 지우기" }));
  await waitFor(() => expect(clearOAuth2Token).toHaveBeenCalled());
  expect(await screen.findByText("토큰 없음")).toBeTruthy();
});

it("starts an explicit response login only once under StrictMode", async () => {
  vi.mocked(authorizeOAuth2).mockResolvedValue({ state: "valid", expiresAtMs: null, scope: null });
  render(
    <StrictMode>
      <OAuth2Editor
        auth={auth}
        environment={environment}
        onChange={vi.fn()}
        loginRequest={1}
        onLoginHandled={vi.fn()}
      />
    </StrictMode>,
  );
  await waitFor(() => expect(authorizeOAuth2).toHaveBeenCalledTimes(1));
  expect(await screen.findByText("유효 · 만료 시각 없음")).toBeTruthy();
});
it("cancels a replaced profile and ignores its late login result", async () => {
  let finish!: (value: Awaited<ReturnType<typeof authorizeOAuth2>>) => void;
  vi.mocked(authorizeOAuth2).mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const { rerender } = render(<OAuth2Editor auth={auth} environment={environment} onChange={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "로그인" }));
  await screen.findByText("브라우저에서 로그인을 마쳐 주세요.");
  const requestId = vi.mocked(authorizeOAuth2).mock.calls[0][0];
  rerender(
    <OAuth2Editor
      auth={{ ...auth, oauth2: { ...config, clientId: "other" } }}
      environment={environment}
      onChange={vi.fn()}
    />,
  );
  expect(cancelOAuth2).toHaveBeenCalledWith(requestId);
  await act(async () => finish({ state: "valid", expiresAtMs: null, scope: null }));
  expect(screen.queryByText("유효 · 만료 시각 없음")).toBeNull();
});
