import type { WorkspaceCapabilities } from "../types";

interface WorkspaceScopeNoteProps {
  workspaceFolder: string | null;
  fileCount: number;
  workspaceTruncated: boolean;
  workspaceIncomplete: boolean;
  workspaceCapabilities: WorkspaceCapabilities | null;
}

export function WorkspaceScopeNote({
  workspaceFolder,
  fileCount,
  workspaceTruncated,
  workspaceIncomplete,
  workspaceCapabilities,
}: WorkspaceScopeNoteProps) {
  return (
    <p className="scope-note">
      작업 폴더: {workspaceFolder ?? "지정되지 않음"} · {fileCount}개 파일
      {workspaceTruncated && " · 일부 목록만 표시"}
      {workspaceIncomplete && " · 일부 항목 읽기 실패"}
      {workspaceCapabilities?.sourceKind === "wsl" &&
        ` · WSL · 5초 폴링 · 편집 가능 · ${workspaceCapabilities.lspSupported ? "WSL 언어 서버" : "호스트 LSP 미지원"}`}
      {workspaceCapabilities?.sourceKind === "native" && " · 네이티브 파일 감시"}
    </p>
  );
}
