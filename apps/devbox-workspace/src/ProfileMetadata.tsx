import type { ProjectProfile } from "@devbox/workspace-features/overview-types";
export type ImportedProfile = {
  id: string;
  sourceSnapshotId?: string | null;
  local?: boolean;
  sourceTemplateId?: string | null;
  profile: ProjectProfile;
};
export type ProfileBinding = { importedId: string; target: "windows" | "wsl"; worktreeId: string };
export type ProfileTemplate = Omit<ProjectProfile, "environment">;
export type ImportedTemplate = {
  id: string;
  sourceSnapshotId?: string | null;
  local?: boolean;
  archived?: boolean;
  template: ProfileTemplate;
};
export function ProfileMetadata({ profile }: { profile: ProjectProfile }) {
  return (
    <dl>
      <dt>Windows 폴더</dt>
      <dd>{profile.windowsPath ?? "없음"}</dd>
      <dt>WSL 폴더</dt>
      <dd>{profile.wsl ? `${profile.wsl.distro}: ${profile.wsl.path}` : "없음"}</dd>
      <dt>Git 폴더</dt>
      <dd>{profile.gitRoot ?? "없음"}</dd>
      <dt>사용 포트</dt>
      <dd>{profile.expectedPorts.join(", ") || "없음"}</dd>
      <dt>서비스 참조</dt>
      <dd>{profile.runManagerServiceIds.join(", ") || "없음"}</dd>
      <dt>환경 설정</dt>
      <dd>
        {profile.environment ? (
          <>
            <span>
              {profile.environment.source} · {profile.environment.enabled ? "기존 활성 설정" : "기존 비활성 설정"}
            </span>
            <ul>
              {profile.environment.variables.map((variable) => (
                <li key={variable.name}>
                  {variable.name} · {variable.source}
                  {variable.secretReference ? ` · 보관된 참조: ${variable.secretReference.name}` : ""}
                  {variable.conflict !== "none" ? " · 충돌 확인 필요" : ""}
                </li>
              ))}
            </ul>
          </>
        ) : (
          "없음"
        )}
      </dd>
    </dl>
  );
}
export function TemplateMetadata({ template }: { template: ProfileTemplate }) {
  return <ProfileMetadata profile={{ ...template, environment: null }} />;
}
