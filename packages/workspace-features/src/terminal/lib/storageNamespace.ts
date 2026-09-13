import { componentInvoke, isProductHosted } from "../../transport";
const invoke=componentInvoke("workspace.terminal");

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

let preferences: Record<string,string>|undefined;
let persisted: Record<string,string>={};
let pending: Promise<void>=Promise.resolve();
export async function initializeTerminalPreferences():Promise<void> {
  preferences=await invoke<Record<string,string>>("terminal_preferences", {});
  for(const key of ["wsl-desktop:cwd-pinned","wsl-desktop:cwd-value","wsl-desktop:recent-paths","wsl-desktop:copy-on-select","wsl-desktop:font-size","wsl-desktop:settings"]) {
    const previous=localStorage.getItem(terminalStorageKey(key));
    if(preferences[key]===undefined && previous!==null) {
      try {await invoke("set_terminal_preference",{key,expected:null,value:previous});preferences[key]=previous;}
      catch { preferences=await invoke<Record<string,string>>("terminal_preferences",{}); }
    }
  }
  persisted={...preferences};
}
export function readTerminalPreference(key:string):string|null {
  if(!isProductHosted())return localStorage.getItem(key);
  if(!preferences)throw new Error("터미널 설정이 준비되지 않았습니다.");
  return preferences[key]??null;
}
export function writeTerminalPreference(key:string,value:string):void {
  if(!isProductHosted()){localStorage.setItem(key,value);return;}
  if(!preferences)throw new Error("터미널 설정이 준비되지 않았습니다.");
  preferences[key]=value;
  pending=pending.then(async()=>{
    try { await invoke("set_terminal_preference", {key,expected:persisted[key]??null,value});persisted[key]=value; }
    catch {
      // A stale companion cannot overwrite a newer import or another window.
      try { persisted=await invoke<Record<string,string>>("terminal_preferences", {});preferences={...persisted}; } catch { /* keep the last confirmed values */ }
      window.dispatchEvent(new Event("terminal-preference-save-failed"));
    }
  });
}
