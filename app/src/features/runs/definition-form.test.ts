import type { Definition, DefinitionList } from "../../rpc/gen/rpc";
import { buildDefinition, initialDefinitionForm, upsertDefinition } from "./definition-form";

const service: Definition = {
  source: "projectFile",
  def: {
    kind: "service",
    id: "web",
    command: "pnpm dev",
    cwd: "src",
    env: { PORT: "3000", EMPTY: "" },
    target: "windows",
    dependsOn: ["db"],
    worktree: "/fixture/worktree",
    secrets: ["TOKEN"],
    restart: "onFailure",
    restartSec: 2,
    startLimit: { burst: 3, intervalSec: 30 },
    portsHint: [3000, 8080],
    readyTimeoutSec: 10,
  },
};
test("editing preserves every service setting and the existing identifier", () => {
  const form = initialDefinitionForm(service);
  expect(form.saveTo).toBe("project");
  expect(buildDefinition(form)).toEqual(service.def);
});
test("schedule fields and source round trip independently of executable defaults", () => {
  const schedule: Definition = {
    source: "thisPc",
    def: { kind: "schedule", id: "nightly", task: "build", cron: "0 9 * * *", tz: "Asia/Seoul", missed: "runOnce" },
  };
  expect(buildDefinition(initialDefinitionForm(schedule))).toEqual(schedule.def);
});
test("suggestion becomes an editable draft with a unique stable identity", () => {
  const form = initialDefinitionForm(undefined, {
    id: "dev",
    command: "pnpm dev",
    kind: "service",
    from: "package.json",
  });
  expect(form.command).toBe("pnpm dev");
  expect(form.kind).toBe("service");
  expect(form.id).toBeTruthy();
  expect(buildDefinition(form).id).toBe(form.id);
});
test("invalid and duplicate environment keys cannot silently discard entered values", () => {
  const form = initialDefinitionForm(service);
  form.env = [
    { id: "1", key: "PORT", value: "3000" },
    { id: "2", key: "PORT", value: "4000" },
  ];
  expect(() => buildDefinition(form)).toThrow("중복");
  form.env = [{ id: "1", key: "A=B", value: "value" }];
  expect(() => buildDefinition(form)).toThrow("환경 변수");
});
test.each(["abc", "1.5", "65536", "-1"])("invalid port %s is rejected instead of converted", (ports) => {
  const form = initialDefinitionForm(service);
  form.ports = ports;
  expect(() => buildDefinition(form)).toThrow("포트");
});
test("saving a local definition preserves project-file precedence and previous run details", () => {
  const local: Definition = { ...service, source: "thisPc" };
  const old: DefinitionList = {
    rev: "1",
    defs: [service],
    shadowed: [local],
    runs: [],
    trust: "approval_required",
    nextCursor: null,
  };
  const changed = { ...local, def: { ...local.def, command: "pnpm start" } } as Definition;
  const result = upsertDefinition(old, { rev: "2", definition: changed });
  expect(result.defs).toEqual([service]);
  expect(result.shadowed).toEqual([changed]);
  expect(result.trust).toBe(old.trust);
  expect(result.runs).toBe(old.runs);
});

test("saving a project definition shadows an existing local definition", () => {
  const local: Definition = { ...service, source: "thisPc" };
  const old: DefinitionList = {
    rev: "1",
    defs: [local],
    shadowed: [],
    runs: [],
    trust: "not_required",
    nextCursor: null,
  };
  const result = upsertDefinition(old, { rev: "2", definition: service });
  expect(result.defs).toEqual([service]);
  expect(result.shadowed).toEqual([local]);
});

test("a late save response does not overwrite a newer authoritative list", () => {
  const old: DefinitionList = {
    rev: "9007199254740994",
    defs: [service],
    shadowed: [],
    runs: [],
    trust: "approved",
    nextCursor: null,
  };
  const changed = { ...service, def: { ...service.def, command: "old command" } } as Definition;
  expect(upsertDefinition(old, { rev: "9007199254740993", definition: changed })).toBe(old);
});
