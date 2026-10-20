import { decodeFrame } from "../../rpc/frame";
import type { Definition, DefinitionList, Run } from "../../rpc/gen/rpc";
import { ProjectWire } from "../projects/test-wire";
export const serviceDefinition: Definition = {
  source: "thisPc",
  def: {
    kind: "service",
    id: "web",
    command: "python3 -m http.server $DEVBOX_PORT",
    cwd: null,
    env: {},
    target: "linux",
    dependsOn: [],
    worktree: null,
    secrets: [],
    restart: "no",
    restartSec: 1,
    startLimit: { burst: 3, intervalSec: 60 },
    portsHint: [],
    readyTimeoutSec: 10,
  },
};
export const runFixture: Run = {
  id: "run-1",
  projectId: "p",
  defId: "web",
  kind: "service",
  state: "running",
  exit: null,
  startedMs: 1,
  endedMs: null,
  restarts: 0,
  unit: "owned.service",
  ready: true,
  pid: 111,
  port: 8123,
  attemptId: "attempt-1",
};
export class RunWire extends ProjectWire {
  rev = "1";
  defs = [serviceDefinition];
  runs: Run[] = [{ ...runFixture }];
  selected: Run = { ...runFixture };
  trust: DefinitionList["trust"] = "not_required";
  containers = [
    { id: "a".repeat(64), name: "owned-container", image: "fake", state: "running", status: "Up", ports: "" },
  ];
  override send(frame: Uint8Array) {
    super.send(frame);
    const request = JSON.parse(new TextDecoder().decode(decodeFrame(frame).body));
    let result: unknown;
    switch (request.method) {
      case "runtime.list":
        result = { rev: this.rev, defs: this.defs, shadowed: [], runs: this.runs, trust: this.trust, nextCursor: null };
        break;
      case "runtime.get":
        result = { rev: this.rev, run: this.selected };
        break;
      case "runtime.containers":
        result = { available: true, items: this.containers };
        break;
      case "runtime.schedules":
        result = { items: [] };
        break;
      case "observe.ports":
        result = [
          {
            port: 8123,
            proto: "tcp",
            addr: "127.0.0.1",
            pid: 111,
            process: "owned",
            runId: "run-1",
            containerId: null,
            projectId: "p",
          },
        ];
        break;
      case "observe.processes":
        result = [
          {
            pid: 111,
            name: "owned",
            uid: 1000,
            cwd: "/owned",
            startTimeMs: 1,
            cpuPercent: 1.5,
            rssBytes: 1048576,
            unit: "owned.service",
            runId: "run-1",
            projectId: "p",
          },
        ];
        break;
      case "logs.open":
        result = { streamId: 100 + this.requests("logs.open").length };
        break;
      default:
        return;
    }
    queueMicrotask(() => this.deliver({ type: "response", id: request.id, result }));
  }
  changed(rev: string) {
    this.rev = rev;
    const subscription = this.sent.find(
      (message) => message.type === "subscribe" && message.topic === "runtime.changed",
    );
    if (!subscription) throw new Error("missing runtime topic subscription");
    this.deliver({ type: "event", subId: subscription.id, rev, payload: { rev, projectId: "p" } });
  }
  reject(method: string, code: string) {
    this.deliver({ type: "response", id: this.request(method, -1).id, error: { code, detail: { id: "p" } } });
  }
}
