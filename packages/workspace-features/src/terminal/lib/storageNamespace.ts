import { isProductHosted } from "../../transport";

let owner: {installation:string;session:string}|undefined;
/** Supplied by the native companion description before the Terminal view loads. */
export function configureTerminalStorage(installation:string,session:string):void {
  if(owner||![installation,session].every(value=>/^[A-Za-z0-9_-]{1,128}$/.test(value)))throw new Error("터미널 저장 위치를 확인하지 못했습니다.");
  owner={installation,session};
}
export function terminalStorageKey(legacy:string):string {
  if(!isProductHosted())return legacy;
  if(!owner)throw new Error("터미널 저장 위치를 확인하지 못했습니다.");
  const scope=legacy.endsWith(":last-layout")?owner.session:"preferences";
  return `devbox-workspace:${owner.installation}:terminal:${scope}:${legacy}`;
}
