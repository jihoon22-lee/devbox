import {afterEach, beforeEach, expect, it, vi} from "vitest";
import {cleanup, fireEvent, render, screen, waitFor} from "@testing-library/react";
import LegacyReferenceLookup from "./LegacyReferenceLookup";
import type {Registry} from "./RegistryGate";
import {nativeCall} from "./native";
vi.mock("./native", () => ({nativeCall: vi.fn()}));
const call = vi.mocked(nativeCall);
const windows = {projectId: "project", worktreeId: "windows", revision: 2, target: {kind: "windows" as const}};
const wsl = {projectId: "project", worktreeId: "wsl", revision: 3, target: {kind: "wsl" as const, distroId: "registered"}};
const registry: Registry = {revision: 5, projects: [{id: "project", name: "합성 프로젝트"}], worktrees: [
  {id: windows.worktreeId, projectId: "project", revision: 2, binding: {root: "C:/fixture", target: windows.target}, trustedDigest: null},
  {id: wsl.worktreeId, projectId: "project", revision: 3, binding: {root: "/한글/Project", target: wsl.target}, trustedDigest: null},
]};
const imported = {id: "imported-one", sourceSnapshotId: "source", profile: {id: "preserved-id", name: "합성 프로젝트", windowsPath: "C:/fixture", wsl: {distro: "Fixture", path: "/한글/Project"}, gitRoot: null, expectedPorts: [], runManagerServiceIds: [], environment: null}};
const candidate = (context: typeof windows | typeof wsl) => ({context, origins: [{kind: "importedProfile", importedId: imported.id}]});
afterEach(cleanup);
beforeEach(() => {call.mockReset();});
it("returns all target candidates and never selects a unique or ambiguous result automatically", async () => {
  call.mockResolvedValue({schemaVersion: 1, registryRevision: 5, state: "ambiguous", candidates: [candidate(windows), candidate(wsl)]});
  const select = vi.fn();
  render(<LegacyReferenceLookup registry={registry} imported={imported} disabled={false} onSelect={select}/>);
  expect(call).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", {name: "참조 연결 조회"}));
  await screen.findByText(/여러 프로젝트가 연결/);
  expect(call).toHaveBeenCalledWith("workspace.registry", "resolve_legacy_reference", {registryRevision: 5, owner: "workbench", oldId: "preserved-id", target: null, importedId: null});
  expect(select).not.toHaveBeenCalled();
  fireEvent.click(screen.getAllByRole("button", {name: "이 연결의 프로젝트 선택"})[1]);
  expect(select).toHaveBeenCalledWith(wsl);
  call.mockResolvedValue({schemaVersion: 1, registryRevision: 5, state: "resolved", candidates: [candidate(windows)]});
  fireEvent.click(screen.getByLabelText("이 보관 항목만 확인"));
  expect(screen.queryByText(/여러 프로젝트가 연결/)).toBeNull();
  fireEvent.click(screen.getByRole("button", {name: "참조 연결 조회"}));
  await screen.findByText(/연결된 프로젝트를 확인했습니다/);
  expect(call.mock.calls[1][2]).toMatchObject({importedId: imported.id});
  expect(select).toHaveBeenCalledTimes(1);
});
it("keeps unmapped references and rejects a stale response without making a selection available", async () => {
  const select = vi.fn();
  call.mockResolvedValue({schemaVersion: 1, registryRevision: 5, state: "unmapped", candidates: []});
  const view = render(<LegacyReferenceLookup registry={registry} imported={imported} disabled={false} onSelect={select}/>);
  fireEvent.click(screen.getByRole("button", {name: "참조 연결 조회"}));
  await screen.findByText(/프로필과 기존 ID는 보존/);
  call.mockResolvedValue({schemaVersion: 1, registryRevision: 4, state: "resolved", candidates: [candidate(windows)]});
  fireEvent.click(screen.getByRole("button", {name: "참조 연결 조회"}));
  await screen.findByRole("alert");
  expect(screen.queryByRole("button", {name: "이 연결의 프로젝트 선택"})).toBeNull();
  view.rerender(<LegacyReferenceLookup registry={registry} imported={imported} disabled onSelect={select}/>);
  expect(screen.getByRole<HTMLButtonElement>("button", {name: "참조 연결 조회"}).disabled).toBe(true);
  expect(select).not.toHaveBeenCalled();
});
it("ignores a completed lookup after its registry revision was replaced", async () => {
  let finish!: (value: unknown) => void;
  call.mockImplementation(() => new Promise(resolve => {finish = resolve;}));
  const view = render(<LegacyReferenceLookup key="5" registry={registry} imported={imported} disabled={false} onSelect={vi.fn()}/>);
  fireEvent.click(screen.getByRole("button", {name: "참조 연결 조회"}));
  view.rerender(<LegacyReferenceLookup key="6" registry={{...registry, revision: 6}} imported={imported} disabled={false} onSelect={vi.fn()}/>);
  finish({schemaVersion: 1, registryRevision: 5, state: "resolved", candidates: [candidate(windows)]});
  await waitFor(() => expect(screen.queryByRole("button", {name: "이 연결의 프로젝트 선택"})).toBeNull());
});
