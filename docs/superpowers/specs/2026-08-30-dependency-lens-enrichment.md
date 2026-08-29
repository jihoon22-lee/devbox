# Dependency Lens Remote Enrichment Design

## Status

Approved implementation contract for the second PR boundary of GitHub issue #484.

## Goal

Add optional vulnerability, license, deprecation, and default-version metadata to the
Repo Manager Dependency Lens without weakening the offline lock graph, exposing repository
context, or making a remote service a prerequisite for local analysis.

## User flow

1. The user runs the existing offline dependency analysis.
2. The user chooses OSV, deps.dev, or both and selects **Review transmission**.
3. Repo Manager performs another bounded local scan and shows the exact, service-specific
   package coordinates that would leave the machine. No network call occurs during preview.
4. The user selects **Send reviewed coordinates**. The backend consumes a one-time preview
   token, revalidates the selected repository and lock revision, and performs only the calls
   represented by that preview.
5. Remote data is rendered beside the corresponding local package nodes. A remote failure,
   partial response, cache failure, or stale fallback never clears or invalidates the local
   graph.

Changing repository, rerunning offline analysis, changing the service selection, or letting
the preview expire requires a new preview.

## Data minimization

Only these public package coordinates may be transmitted:

- OSV: ecosystem, package name, exact resolved version.
- deps.dev version lookup: system, package name, exact resolved version.
- deps.dev package lookup: system and package name, to identify the service's declared default
  version.

The following never leave the machine:

- repository path or opaque project ID;
- manifest or lockfile path/content/revision;
- dependency graph edges and direct/transitive topology;
- registry/source URL, integrity/checksum, Git URL, credentials, environment, or user identity.

The preview groups exact coordinates under `api.osv.dev` and `api.deps.dev`. Cache hits are
shown separately and are not described as network transmissions. A force-refresh option may
turn a fresh cache hit back into an explicitly previewed transmission.

## Service mapping and limits

| Local ecosystem | OSV ecosystem | deps.dev system |
| --- | --- | --- |
| Cargo | `crates.io` | `CARGO` |
| pnpm | `npm` | `NPM` |
| npm | `npm` | `NPM` |
| Python/uv | `PyPI` | `PYPI` |
| Gradle | unsupported until the local parser resolves Maven coordinates | unsupported |

- OSV uses `POST https://api.osv.dev/v1/querybatch` for at most 256 unique resolved
  coordinates. Results remain aligned to request order. Pagination is not followed; a page
  token is surfaced as a bounded partial result.
- deps.dev uses stable v3 `GetVersion` and `GetPackage` endpoints for at most 48 unique direct
  coordinates. No transitive coordinate is disclosed to deps.dev in this release.
- pnpm and npm nodes that map to the same remote coordinate are deduplicated before transmission
  and mapped back to every matching local package ID.
- Redirects are disabled. Production endpoints and schemes are fixed in native code; the IPC
  request accepts no URL, header, service credential/auth token, query, body override, or proxy
  setting. The separate opaque preview token only identifies the stored one-time review plan.
- Per-request timeout is four seconds, dependency metadata runs in bounded groups of four
  coordinates (at most eight simultaneous GETs), and only one enrichment execution may run in
  the process.
- Response bodies are read incrementally under explicit byte limits before JSON parsing.
  Package names, versions, licenses, and advisory IDs are validated and bounded before IPC.

Official contracts used for this design:

- <https://google.github.io/osv.dev/post-v1-querybatch/>
- <https://ossf.github.io/osv-schema/#affectedpackage-field>
- <https://docs.deps.dev/api/v3/>

## Preview integrity

The native process stores at most eight previews for five minutes. A preview contains the
validated canonical repository identity, lock revision, exact service plans, cache decisions,
and expiry. The UI receives an opaque token and a renderable copy of the outbound plan.

Execution accepts only the selected repository path and preview token. It consumes the token
once, reopens and revalidates the repository, reruns the bounded scanner, and requires the same
canonical identity and revision. It cannot add a coordinate or service after review. A cache
race may not expand transmission; fresh previewed cache data remains usable until preview
expiry.

## Cache and freshness

- App-owned path: `%LOCALAPPDATA%/devbox/repo-manager/dependency-enrichment-v1.json`.
- Linux tests use an injected temporary root; production uses the common devbox root.
- Cache keys are SHA-256 digests of the normalized remote coordinate. Package names,
  repository paths, and project identities are not stored in the cache file.
- Each service result has its own fetch timestamp and value so partial success remains useful.
- Fresh TTL: 24 hours. Fresh entries avoid network unless force refresh was reviewed.
- Stale fallback: up to seven days. If a reviewed refresh fails, stale data may be returned only
  with an explicit `stale` status and age.
- Older, malformed, oversized, symlinked/reparse-point, unknown-schema, or over-count cache data
  fails closed and is treated as a cache miss. Writes use bounded serialization and atomic
  replacement. Cache read/write failure does not fail offline analysis or a successful remote
  response.
- At most 2,048 coordinate entries and four MiB are retained. Oldest entries are pruned first.

## Result contract

Each returned coordinate contains the matching local package IDs and separate OSV/deps.dev
states: `fresh`, `cached`, `stale`, `failed`, or `notRequested`.

- OSV data: bounded advisory IDs and an explicit pagination-truncated flag.
- deps.dev data: bounded SPDX-like license strings, service default version when known,
  deprecation flag, and direct advisory IDs as corroborating metadata.
- The UI labels license data as informational rather than legal advice and default version as
  the service's package-manager-specific default, not a guaranteed safe upgrade.
- Remote errors are fixed local error codes/messages. Response bodies, URLs, headers, server
  text, and native error chains are never reflected to the renderer.

Remote results are Repo Manager-local derived data. They are not added to
`dependency-summary/v1`, so Workbench remains an offline aggregate consumer and package-level
remote metadata does not cross the integration boundary.

## Tests and completion

- Pure tests cover ecosystem mapping, direct-only selection, deduplication, caps, preview/cache
  freshness, one-time expiry rules, strict cache validation, response bounds, and result merge.
- Native fixture tests cover exact OSV/deps.dev requests, disabled redirects, timeout/partial
  failure, stale fallback, revision mismatch, and cache isolation without contacting the public
  Internet.
- Frontend tests cover no-network preview, exact disclosure rendering, explicit confirmation,
  service selection, stale/partial states, repository navigation and offline-rerun invalidation,
  and preservation of the local report after remote failure.
- Workspace Rust check, Clippy, format, tests, frontend build/tests/typecheck, dependency policy,
  and Windows Rust CI must pass before merge.
