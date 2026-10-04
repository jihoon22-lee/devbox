// Identity discovery is read-only. A live handle without verified identity is
// preserved, never terminated through an arbitrary rediscovered process ID.
export async function cleanupOwnedFixture(item, stop, release) {
  await withOwnedCleanup(
    async () => {
      if (item.child?.exitCode === null && item.child.signalCode == null) {
        if (!item.identity) throw new Error("Owned process identity unavailable; fixture preserved");
        await stop();
      }
    },
    async () => {
      try {
        await release();
      } finally {
        if (item.child?.exitCode === null && item.child.signalCode == null) {
          try {
            item.child.stdin?.destroy();
            item.child.stdout?.destroy();
            item.child.stderr?.destroy();
          } finally {
            item.child.unref();
          }
        }
      }
    },
  );
}
export async function withOwnedCleanup(work, cleanup) {
  let failed = false;
  try {
    return await work();
  } catch (error) {
    failed = true;
    throw error;
  } finally {
    try {
      await cleanup();
    } catch (error) {
      if (!failed) throw error;
    }
  }
}
