// Only an explicitly identified retained diagnosis may continue past required gates.
export async function runKnowledgeChecks({ diagnosticOnly, checks, isUsable, onFirstFailure }) {
  let firstError = null;
  const completed = [],
    failed = [];
  for (const check of checks) {
    if (firstError && !isUsable()) break;
    try {
      await check.run();
      completed.push(check.name);
    } catch (error) {
      if (diagnosticOnly !== true) throw error;
      failed.push(check.name);
      if (!firstError) {
        firstError = error;
        await onFirstFailure(error);
      }
    }
  }
  return { firstError, completed, failed };
}
