import type {
  DefineResult,
  Definition,
  DefinitionList,
  MissedPolicy,
  RestartPolicy,
  RunDef,
  RunTarget,
  SaveTo,
  Suggestion,
} from "../../rpc/gen/rpc";
import { compareRev } from "../../rpc/revisions";
import type { KeyValueRow } from "../../ui/KeyValueEditor";

export interface DefinitionForm {
  id: string;
  kind: RunDef["kind"];
  command: string;
  cwd: string;
  worktree: string;
  env: KeyValueRow[];
  target: RunTarget;
  dependencies: string;
  secrets: string;
  restart: RestartPolicy;
  restartSec: string;
  burst: string;
  intervalSec: string;
  ports: string;
  readyTimeoutSec: string;
  task: string;
  cron: string;
  tz: string;
  missed: MissedPolicy;
  saveTo: SaveTo;
}
export function initialDefinitionForm(definition?: Definition, suggestion?: Suggestion): DefinitionForm {
  const def = definition?.def;
  const executable = def && def.kind !== "schedule" ? def : undefined;
  const service = def?.kind === "service" ? def : undefined;
  const schedule = def?.kind === "schedule" ? def : undefined;
  return {
    id: def?.id ?? crypto.randomUUID(),
    kind: def?.kind ?? suggestion?.kind ?? "task",
    command: executable?.command ?? suggestion?.command ?? "",
    cwd: executable?.cwd ?? "",
    worktree: executable?.worktree ?? "",
    target: executable?.target ?? "linux",
    env: Object.entries(executable?.env ?? {}).map(([key, value]) => ({ id: crypto.randomUUID(), key, value })),
    dependencies: executable?.dependsOn.join(", ") ?? "",
    secrets: executable?.secrets.join(", ") ?? "",
    restart: service?.restart ?? "no",
    restartSec: String(service?.restartSec ?? 1),
    burst: String(service?.startLimit.burst ?? 5),
    intervalSec: String(service?.startLimit.intervalSec ?? 60),
    ports: service?.portsHint.join(", ") ?? "",
    readyTimeoutSec: String(service?.readyTimeoutSec ?? 60),
    task: schedule?.task ?? "",
    cron: schedule?.cron ?? "",
    tz: schedule?.tz ?? "",
    missed: schedule?.missed ?? "skip",
    saveTo: definition?.source === "thisPc" ? "thisPc" : "project",
  };
}
const items = (value: string) =>
  value
    .split(/[,\n]/)
    .map((item) => item.trim())
    .filter(Boolean);
function integer(value: string, label: string, max = 4294967295): number {
  if (!/^\d+$/.test(value) || Number(value) > max) throw new Error(`${label}: 0부터 ${max}까지 정수를 입력하세요.`);
  return Number(value);
}
export function buildDefinition(form: DefinitionForm): RunDef {
  const { id, kind } = form;
  if (!id.trim() || new TextEncoder().encode(id).length > 128 || id.includes("\0"))
    throw new Error("작업 이름은 빈칸이 아닌 128바이트 이내로 입력하세요.");
  if (kind === "schedule") {
    if (!form.task.trim() || !form.cron.trim()) throw new Error("예약할 작업과 예약 식을 입력하세요.");
    return {
      id,
      kind,
      task: form.task.trim(),
      cron: form.cron.trim(),
      tz: form.tz.trim() || null,
      missed: form.missed,
    };
  }
  if (!form.command.trim()) throw new Error("명령을 입력하세요.");
  const entries = form.env.map(({ key, value }) => [key.trim(), value] as const);
  const names = entries.map(([key]) => key);
  if (names.some((name) => !name || /[=\0]/.test(name)) || new Set(names).size !== names.length)
    throw new Error("환경 변수 이름은 비어 있거나 중복될 수 없고 = 문자를 포함할 수 없습니다.");
  const common = {
    id,
    command: form.command,
    cwd: form.cwd.trim() || null,
    worktree: form.worktree.trim() || null,
    env: Object.fromEntries(entries),
    target: form.target,
    dependsOn: items(form.dependencies),
    secrets: items(form.secrets),
  };
  if (kind === "task") return { ...common, kind };
  return {
    ...common,
    kind,
    restart: form.restart,
    restartSec: integer(form.restartSec, "재시작 대기"),
    startLimit: { burst: integer(form.burst, "재시작 한도"), intervalSec: integer(form.intervalSec, "한도 구간") },
    portsHint: items(form.ports).map((port) => integer(port, "포트", 65535)),
    readyTimeoutSec: integer(form.readyTimeoutSec, "준비 대기"),
  };
}
export function upsertDefinition(old: DefinitionList | undefined, saved: DefineResult): DefinitionList {
  if (old && compareRev(old.rev, saved.rev) > 0) return old;
  const list = old ?? { rev: saved.rev, defs: [], shadowed: [], runs: [], trust: "not_required", nextCursor: null };
  const sameId = (definition: Definition) => definition.def.id === saved.definition.def.id;
  const current = list.defs.find(sameId);
  // Project-file definitions take precedence over this-PC definitions with the same ID.
  const shadowedSave = saved.definition.source === "thisPc" && current?.source === "projectFile";
  const defs = shadowedSave ? list.defs : [...list.defs.filter((def) => !sameId(def)), saved.definition];
  const shadowed = list.shadowed.filter((def) => !(sameId(def) && def.source === saved.definition.source));
  if (shadowedSave) shadowed.push(saved.definition);
  else if (current && current.source !== saved.definition.source) shadowed.push(current);
  return { ...list, rev: saved.rev, defs, shadowed };
}
