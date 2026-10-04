import { appendFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

// rust-cache v2 prefixes shared-key with prefix-key (default v0-rust).
// Do not reproduce its environment hashes, archive version, or cached paths.
export async function needsLegacyWarmStart({ repository, shard, ref, token, fetchCache = fetch }) {
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository ?? "") || !/^\d{2}$/.test(shard ?? "") || ref !== "refs/heads/main") {
    throw new Error("Invalid candidate cache scope");
  }
  const prefix = `v0-rust-devbox-windows-package-shard-${shard}-`;
  const url = new URL(`https://api.github.com/repos/${repository}/actions/caches`);
  url.search = new URLSearchParams({ key: prefix, ref, per_page: "1" });
  try {
    const response = await fetchCache(url, {
      headers: {
        Accept: "application/vnd.github+json",
        Authorization: `Bearer ${token}`,
        "X-GitHub-Api-Version": "2022-11-28",
      },
      signal: AbortSignal.timeout(10_000),
    });
    if (!response.ok) return true;
    const result = await response.json();
    if (
      !Number.isSafeInteger(result?.total_count) ||
      result.total_count < 1 ||
      !Array.isArray(result.actions_caches) ||
      result.actions_caches.length !== 1
    ) {
      return true;
    }
    const cache = result.actions_caches[0];
    return !(typeof cache?.key === "string" && cache.key.startsWith(prefix) && cache.ref === ref);
  } catch {
    // Metadata is an optimization. Preserve the existing warm restore on failure.
    return true;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const warmStart = await needsLegacyWarmStart({
    repository: process.env.GITHUB_REPOSITORY,
    shard: process.env.CACHE_SHARD,
    ref: process.env.GITHUB_REF,
    token: process.env.GITHUB_TOKEN,
  });
  appendFileSync(process.env.GITHUB_OUTPUT, `legacy-warm-start=${warmStart}\n`);
}
