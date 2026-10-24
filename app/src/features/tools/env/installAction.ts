import { invoke } from "@tauri-apps/api/core";
import type { InstallPlan, Methods } from "../../../rpc/gen/rpc";
export type ExecutablePlan = Extract<InstallPlan, { kind: "wsl" | "windows" }>;
export const WINDOWS_INSTALL_COMMAND = "winget.exe install --id Docker.DockerDesktop -e --source winget";
export function installationDetails(plan: ExecutablePlan) {
  return plan.kind === "wsl"
    ? {
        title: plan.title,
        command: plan.shellCommand,
        body: "새 터미널에서 바로 실행됩니다(sudo 비밀번호를 물을 수 있음). 실행할 명령을 확인해 주세요.",
      }
    : {
        title: "Docker Desktop 설치",
        command: WINDOWS_INSTALL_COMMAND,
        body: "Windows의 새 설치 창에서 바로 실행됩니다. 설치 도중 권한을 물을 수 있습니다.",
      };
}
/** Called only by the explicit confirmation action. Never execute on plan retrieval. */
export async function executeInstallation(
  plan: ExecutablePlan,
  create: (params: Methods["terminal.create"]["params"]) => Promise<Methods["terminal.create"]["result"]>,
) {
  if (plan.kind === "windows") {
    await invoke("install_windows_tool", { package: plan.package });
    return null;
  }
  return create({ projectId: null, cwd: null, title: plan.title, command: plan.shellCommand, attachZellij: null });
}
