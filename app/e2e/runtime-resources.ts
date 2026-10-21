import { execFileSync, spawnSync } from "node:child_process";
import { lstatSync, readFileSync, rmdirSync, unlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";

/** Runtime E2Es alone own a transient slice; ordinary UI fixtures need no systemd jobs. */
export class RuntimeResources {
  readonly slice: string;
  private readonly anchor: string;
  private readonly control: string;
  private owned = false;
  private readonly env: NodeJS.ProcessEnv;
  constructor(
    private readonly instance: string,
    env: NodeJS.ProcessEnv,
  ) {
    if (!/^test-[a-f0-9]+$/.test(instance)) throw new Error("Runtime fixture requires a unique test instance");
    const uid = process.getuid?.();
    if (uid === undefined) throw new Error("Runtime fixture requires Linux");
    this.slice = `devbox-${instance}.slice`;
    this.anchor = `devbox-${instance}-fixture.service`;
    this.control = `/run/user/${uid}/systemd/user.control/${this.slice}.d`;
    env.DBUS_SESSION_BUS_ADDRESS = `unix:path=/run/user/${uid}/bus`;
    env.DEVBOX_TEST_SLICE = this.slice;
    delete env.DEVBOX_NO_SCOPE;
    this.env = { PATH: "/usr/bin:/bin", DBUS_SESSION_BUS_ADDRESS: env.DBUS_SESSION_BUS_ADDRESS };
    // Never capture the host login environment or its secret values.
    const fixturePath = `${join(env.HOME!, "bin")}:/usr/local/bin:/usr/bin:/bin`;
    writeFileSync(join(env.XDG_CONFIG_HOME!, "devbox", instance, "env"), `PATH="${fixturePath}"\nLANG="C.UTF-8"\n`);
  }
  private command(program: string, args: string[], allowFailure = false) {
    if (allowFailure) return spawnSync(program, args, { env: this.env, timeout: 15000, encoding: "utf8" });
    return { status: 0, stdout: execFileSync(program, args, { env: this.env, timeout: 15000, encoding: "utf8" }) };
  }
  private units(...names: string[]): { unit: string }[] {
    return JSON.parse(
      this.command("systemctl", ["--user", "list-units", "--all", "--output=json", "--", ...names]).stdout!,
    );
  }
  start() {
    if (this.owned) return;
    if (this.units(this.slice, this.anchor, `devbox-${this.instance}-run-*`).length)
      throw new Error("Fresh runtime resources already exist");
    try {
      lstatSync(this.control);
      throw new Error("Fresh runtime control directory already exists");
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
    }
    this.owned = true;
    try {
      this.command("systemd-run", [
        "--user",
        "--unit",
        this.anchor,
        "--slice",
        this.slice,
        "--collect",
        "--property=Nice=10",
        "--property=MemoryHigh=6G",
        "--property=MemoryMax=8G",
        "--property=MemorySwapMax=1G",
        "--property=CPUQuota=400%",
        "--",
        "/bin/sleep",
        "600",
      ]);
      const allowed = readFileSync("/proc/self/status", "utf8").match(/^Cpus_allowed_list:\s*(.+)$/m)?.[1];
      if (!allowed) throw new Error("CPU affinity unavailable");
      const cpus: number[] = [];
      for (const part of allowed.split(",")) {
        const [start, last = start] = part.split("-").map(Number);
        for (let cpu = start!; cpu <= last! && cpus.length < 4; cpu++) cpus.push(cpu);
      }
      this.command("systemctl", [
        "--user",
        "set-property",
        "--runtime",
        this.slice,
        "MemoryHigh=6G",
        "MemoryMax=8G",
        "MemorySwapMax=1G",
        "CPUQuota=400%",
        `AllowedCPUs=${cpus.join(",")}`,
      ]);
      const properties = this.command("systemctl", ["--user", "show", "--", this.slice]).stdout!;
      for (const expected of ["MemoryHigh=6442450944", "MemoryMax=8589934592", "MemorySwapMax=1073741824"]) {
        if (!properties.split("\n").includes(expected))
          throw new Error(`Runtime resource property missing: ${expected}`);
      }
    } catch (error) {
      this.close();
      throw error;
    }
  }
  close() {
    if (!this.owned) return;
    const prefix = `devbox-${this.instance}-run-`;
    const units = this.units(`${prefix}*`).map((row) => row.unit);
    if (units.some((unit) => !unit.startsWith(prefix))) throw new Error("Unowned runtime unit returned");
    this.command("systemctl", ["--user", "stop", "--", this.slice], true);
    for (const unit of [...units, this.anchor, this.slice]) {
      this.command("systemctl", ["--user", "stop", "--", unit], true);
      this.command("systemctl", ["--user", "reset-failed", "--", unit], true);
    }
    const remaining = this.units(this.anchor, `${prefix}*`).filter((row) => row.unit !== this.slice);
    if (remaining.length)
      throw new Error(`Runtime fixture units remain: ${remaining.map((row) => row.unit).join(", ")}`);
    let metadata;
    try {
      metadata = lstatSync(this.control);
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
    }
    if (metadata) {
      if (!metadata.isDirectory() || metadata.isSymbolicLink())
        throw new Error("Owned runtime control directory was replaced");
      for (const property of ["AllowedCPUs", "CPUQuota", "MemoryHigh", "MemoryMax", "MemorySwapMax"]) {
        try {
          unlinkSync(join(this.control, `50-${property}.conf`));
        } catch (error) {
          if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
        }
      }
      // Unknown entries prevent removal; no recursive systemd cleanup or reload.
      rmdirSync(this.control);
    }
    this.owned = false;
  }
}
