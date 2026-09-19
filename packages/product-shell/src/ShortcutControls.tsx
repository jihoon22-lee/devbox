import type {ShortcutConfig,ShortcutStatus} from "./launcher/types";
export default function ShortcutControls({status,busy,onChange,hideAccelerator=false}:{status:ShortcutStatus;busy:boolean;onChange:(next:ShortcutConfig)=>void;hideAccelerator?:boolean}){
 const config:ShortcutConfig={accelerator:status.accelerator,enabled:status.enabled,...(status.terminal===undefined?{}:{terminal:status.terminal,capture:status.capture,project:status.project})};
 return <>
  {!hideAccelerator&&<label>Launcher 단축키 <select disabled={busy} value={status.accelerator} onChange={event=>onChange({...config,accelerator:event.target.value as ShortcutConfig["accelerator"]})}><option>Ctrl+Alt+Space</option><option>Ctrl+Alt+L</option><option>Ctrl+Alt+J</option></select></label>}
  {typeof status.terminal==="boolean"&&<fieldset disabled={busy}><legend>이 설치의 전역 단축키</legend>
    <label><input type="checkbox" checked={status.enabled} onChange={event=>onChange({...config,enabled:event.target.checked})}/>전역 단축키 사용</label>
    {([['terminal','터미널 표시·숨김 · Ctrl+Alt+T'],['capture','빠른 캡처 · Ctrl+Alt+N'],['project','현재 프로젝트 · Ctrl+Alt+P']] as const).map(([key,label])=><label key={key}><input type="checkbox" checked={status[key]===true} onChange={event=>onChange({...config,[key]:event.target.checked})}/>{label}</label>)}
  </fieldset>}
 </>;
}
