// A ChildProcess owns its native process handle; never terminate a rediscovered
// PID or unrelated descendants. A missing exit after kill cannot block evidence.
export function waitForFixtureChildExit(child, timeoutMs, cleanupMs = 2000) {
  if (child.exitCode !== null || child.signalCode !== null) return Promise.resolve([child.exitCode, child.signalCode]);
  return new Promise((resolve, reject) => {
    let cleanupTimer, timedOut;
    const finish = (error, value) => {
      clearTimeout(timer);
      clearTimeout(cleanupTimer);
      child.removeListener("exit", exited);
      child.removeListener("error", failed);
      error ? reject(error) : resolve(value);
    };
    const exited = (code, signal) => finish(timedOut, [code, signal]);
    const failed = (error) => {
      if (!timedOut) finish(error);
    };
    child.once("exit", exited);
    child.on("error", failed);
    const timer = setTimeout(() => {
      timedOut = Object.assign(new Error("Owned fixture subprocess deadline exceeded"), { code: "ETIMEDOUT" });
      cleanupTimer = setTimeout(() => {
        // Keep the owned namespace for the outer cleanup owner. Release only
        // this runner's pipes/handle so later cases and artifacts can proceed.
        child.stdin?.destroy();
        child.stdout?.destroy();
        child.stderr?.destroy();
        child.unref?.();
        finish(timedOut);
      }, cleanupMs);
      try {
        child.kill("SIGKILL");
      } catch {
        // Failed termination retains the original deadline failure.
      }
    }, timeoutMs);
  });
}
