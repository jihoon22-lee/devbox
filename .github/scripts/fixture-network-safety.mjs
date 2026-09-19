// WSL distro names, separate data roots and Docker socket names do not isolate
// the host network. Only a disposable GitHub-hosted Windows runner is admitted.
export function requireHostedNetworkFixture(env=process.env){
  if(env.GITHUB_ACTIONS!=="true"||env.RUNNER_ENVIRONMENT!=="github-hosted"
    ||env.RUNNER_OS!=="Windows"||env.GITHUB_REPOSITORY!=="jihoon22-lee/devbox"
    ||!/^\d+$/.test(env.GITHUB_RUN_ID??"")){
    throw new Error("Network-changing fixtures are disabled on local and self-hosted machines; use a disposable GitHub-hosted Windows runner. Never spoof CI environment variables.");
  }
  return {runId:env.GITHUB_RUN_ID};
}
