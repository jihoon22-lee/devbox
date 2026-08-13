import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  languageServerStatuses,
  loadLspConfig,
  saveLspConfig,
  startLanguageServer,
  stopLanguageServer,
} from "../api";
import type { LoadedLspConfig } from "../types";
import LspControlPanel from "./LspControlPanel";

vi.mock("../api", () => ({
  languageServerStatuses: vi.fn(),
  loadLspConfig: vi.fn(),
  saveLspConfig: vi.fn(),
  startLanguageServer: vi.fn(),
  stopLanguageServer: vi.fn(),
}));

const loadMock = vi.mocked(loadLspConfig);
const statusesMock = vi.mocked(languageServerStatuses);
const saveMock = vi.mocked(saveLspConfig);
const startMock = vi.mocked(startLanguageServer);
const stopMock = vi.mocked(stopLanguageServer);

function loadedConfig(overrides: Partial<LoadedLspConfig> = {}): LoadedLspConfig {
  return {
    config: {
      version: 1,
      enabled: false,
      workspace_root: "",
      server_by_language: {},
      custom_servers: [],
      update_policy: "manual",
    },
    persist_allowed: true,
    error: null,
    ...overrides,
  };
}

beforeEach(() => {
  loadMock.mockReset().mockResolvedValue(loadedConfig());
  statusesMock.mockReset().mockResolvedValue([]);
  saveMock.mockReset().mockResolvedValue(undefined);
  startMock.mockReset().mockResolvedValue(undefined);
  stopMock.mockReset().mockResolvedValue(undefined);
});

afterEach(() => cleanup());

describe("LspControlPanel", () => {
  it("requires an explicit recovery save for a corrupt config", async () => {
    loadMock.mockResolvedValue(loadedConfig({ persist_allowed: false, error: "invalid JSON" }));
    const rendered = render(<LspControlPanel workspaceRoot={"/work/project"} onClose={() => undefined} />);
    expect(await rendered.findByText(/저장된 설정이 손상되었습니다/)).toBeTruthy();
    fireEvent.click(rendered.getByRole("button", { name: "설정 저장" }));
    await waitFor(() => expect(saveMock).toHaveBeenCalledWith(
      expect.objectContaining({ workspace_root: "/work/project" }),
      true,
    ));
  });

  it("stores local server arguments as argv lines without shell parsing", async () => {
    const rendered = render(<LspControlPanel workspaceRoot={"C:\\work"} onClose={() => undefined} />);
    await rendered.findByText("등록된 언어 서버가 없습니다.");
    await waitFor(() => expect((rendered.getByRole("button", { name: "설정 저장" }) as HTMLButtonElement).disabled).toBe(false));
    fireEvent.change(rendered.getByLabelText("실행 파일 절대 경로"), {
      target: { value: "C:\\Tools\\server.exe" },
    });
    fireEvent.change(rendered.getByLabelText(/인자 \(한 줄에 하나/), {
      target: { value: "--stdio\n--log file=C:\\my logs\\server.log" },
    });
    fireEvent.click(rendered.getByRole("button", { name: "이 언어 설정 적용" }));
    fireEvent.click(rendered.getByRole("checkbox", { name: "이 작업 폴더에서 언어 서버 사용" }));
    fireEvent.click(rendered.getByRole("button", { name: "설정 저장" }));
    await waitFor(() => expect(saveMock).toHaveBeenCalledWith(
      expect.objectContaining({
        enabled: true,
        workspace_root: "C:\\work",
        server_by_language: {
          rust: {
            kind: "local",
            installed_path: "C:\\Tools\\server.exe",
            executable: null,
            args: ["--stdio", "--log file=C:\\my logs\\server.log"],
          },
        },
      }),
      false,
    ));
  });

  it("does not start a server until edited settings are saved", async () => {
    loadMock.mockResolvedValue(loadedConfig({
      config: {
        ...loadedConfig().config,
        enabled: true,
        workspace_root: "C:\\work",
        server_by_language: {
          rust: { kind: "local", installed_path: "C:\\server.exe", args: [] },
        },
      },
    }));
    const rendered = render(<LspControlPanel workspaceRoot={"C:\\work"} onClose={() => undefined} />);
    const start = await rendered.findByRole("button", { name: "시작" });
    await waitFor(() => expect((start as HTMLButtonElement).disabled).toBe(false));
    fireEvent.change(rendered.getByLabelText("실행 파일 절대 경로"), {
      target: { value: "C:\\new-server.exe" },
    });
    fireEvent.click(rendered.getByRole("button", { name: "이 언어 설정 적용" }));
    expect((start as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(start);
    expect(startMock).not.toHaveBeenCalled();
  });
});
