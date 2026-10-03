import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import App from "../App";
import { sanitizePersistedJson } from "../api";
import { seedPreviewDocument, previewDocumentKey } from "../../storage/testDocuments";
const nativeMode = vi.hoisted(() => ({ value: false }));
vi.mock("../lib/isTauri", () => ({ isTauri: () => nativeMode.value }));

vi.mock("../api", () => ({
  ackApiRequest: vi.fn(),
  buildRevealedCurl: vi.fn(),
  claimApiRequest: vi.fn(),
  copyRawResponseCookies: vi.fn(),
  copyRawResponseHeaders: vi.fn(),
  discardCurrentResponse: vi.fn(async () => undefined),
  onOpenRequest: vi.fn(async () => () => undefined),
  readJsonFile: vi.fn(),
  pickCollectionFolder: vi.fn(),
  writeCollectionFolder: vi.fn(),
  renewApiRequest: vi.fn(),
  restoreApiRequest: vi.fn(),
  saveJsonFile: vi.fn(),
  saveResponseBinary: vi.fn(),
  sanitizePersistedJson: vi.fn(),
  sealSecret: vi.fn(),
  oauth2Status: vi.fn(async () => ({ state: "missing", expiresAtMs: null, scope: null })),
  authorizeOAuth2: vi.fn(),
  cancelOAuth2: vi.fn(async () => {}),
  fetchOAuth2Token: vi.fn(),
  clearOAuth2Token: vi.fn(async () => {}),
  revealCapture: vi.fn(),
  discardCaptures: vi.fn(async () => {}),
  restoreCaptures: vi.fn(async () => {}),
  sendSelectionToToolbox: vi.fn(),
  sendRequest: vi.fn(),
  startSseStream: vi.fn(),
  takePendingOpen: vi.fn(async () => null),
}));

afterEach(cleanup);
beforeEach(() => {
  localStorage.clear();
  vi.mocked(sanitizePersistedJson).mockImplementation(async (value) => value);
});
it("adds a variable without overwriting a sparse secret and supports rename and confirmed deletion", async () => {
  seedPreviewDocument(
    "environments",
    JSON.stringify({
      version: 1,
      environments: [
        { id: "e", name: "synthetic dev", variables: [{ key: "var2", value: "sealed-fixture", secret: true }] },
      ],
    }),
  );
  const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "synthetic dev" }));
  const add = await screen.findByRole("button", { name: "+ 변수" });
  await waitFor(() => expect((add as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(add);
  const added = await screen.findByRole("textbox", { name: "환경 변수 2 이름" });
  expect(document.activeElement).toBe(added);
  fireEvent.change(added, { target: { value: "var2" } });
  fireEvent.blur(added);
  expect(await screen.findByText("같은 이름의 변수가 있습니다.")).toBeTruthy();
  fireEvent.change(added, { target: { value: "baseUrl" } });
  fireEvent.blur(added);
  const remove = await screen.findByRole("button", { name: "환경 변수 baseUrl 삭제" });
  fireEvent.click(remove);
  expect(screen.getByRole("button", { name: "환경 변수 baseUrl 삭제" })).toBeTruthy();
  confirm.mockReturnValue(true);
  fireEvent.click(remove);
  await waitFor(() => expect(screen.queryByRole("button", { name: "환경 변수 baseUrl 삭제" })).toBeNull());
  const retained = JSON.parse(localStorage.getItem(previewDocumentKey("environments"))!);
  expect(retained.environments[0].variables).toEqual([{ key: "var2", value: "sealed-fixture", secret: true }]);
  confirm.mockRestore();
});
it("selects the newly created environment by identity when names repeat", async () => {
  seedPreviewDocument(
    "environments",
    JSON.stringify({ version: 1, environments: [{ id: "old", name: "dev", variables: [] }] }),
  );
  render(<App />);
  const name = await screen.findByRole("textbox", { name: "환경 이름" });
  fireEvent.change(name, { target: { value: "dev" } });
  const add = name.parentElement!.querySelector("button")!;
  await waitFor(() => expect(add.disabled).toBe(false));
  fireEvent.click(add);
  await waitFor(() => expect(screen.getAllByRole("button", { name: "dev" })).toHaveLength(2));
  const buttons = screen.getAllByRole("button", { name: "dev" });
  await waitFor(() => expect(buttons[1].getAttribute("aria-pressed")).toBe("true"));
  expect(buttons[0].getAttribute("aria-pressed")).toBe("false");
});
