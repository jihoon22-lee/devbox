export type LegacySource="workbench"|"code-pad"|"code-pad-legacy"|"repo-manager"|"port-manager"|"log-lens"|"wsl-desktop";
export const legacySources:Record<LegacySource,string>={workbench:"Workbench","code-pad":"Code Pad","code-pad-legacy":"Code Pad (이전 버전)","repo-manager":"Repo Manager","port-manager":"Port Manager","log-lens":"Log Lens","wsl-desktop":"WSL Desktop"};
export const isCodePadSource=(source:string)=>source==="code-pad"||source==="code-pad-legacy";
