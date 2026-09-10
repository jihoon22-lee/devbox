import type {
  ListenerIdentity,
  PortFavorite,
  PortManagerPreferences,
  PortRow,
  ProcessFavorite,
} from "./types";

export const DEFAULT_REFRESH_INTERVAL_MS = 5_000;
export const MIN_REFRESH_INTERVAL_MS = 1_000;
export const MAX_REFRESH_INTERVAL_MS = 60_000;
export const MAX_FAVORITES_PER_KIND = 256;
export const MAX_REFRESH_TIMELINE_EVENTS = 256;

export const DEFAULT_PREFERENCES: PortManagerPreferences = {
  schema_version: 1,
  refresh_interval_ms: DEFAULT_REFRESH_INTERVAL_MS,
  pinned_only: false,
  favorite_ports: [],
  favorite_processes: [],
};

function segment(value: string | number): string {
  const text = String(value);
  return text.length + ":" + text;
}

function canonicalProto(value: string): string {
  return value.toUpperCase();
}

export function identityKey(identity: ListenerIdentity): string {
  if (identity.kind === "windows") {
    return ["windows", identity.pid, identity.start_time].map(segment).join("|");
  }
  if (identity.kind === "wsl") {
    return ["wsl", identity.distro, identity.pid, identity.start_tick]
      .map(segment)
      .join("|");
  }
  return ["container", identity.engine, identity.container_id, identity.distro]
    .map(segment)
    .join("|");
}

function endpointKey(row: PortRow): string {
  return ["endpoint", row.source ?? "windows", canonicalProto(row.proto), row.local_addr, row.port]
    .map(segment)
    .join("|");
}

function identityOnlyKey(row: PortRow): string {
  return row.identity ? identityKey(row.identity) : endpointKey(row);
}

function rowFingerprint(row: PortRow): string {
  return JSON.stringify([
    row.source ?? "windows",
    canonicalProto(row.proto),
    row.local_addr,
    row.port,
    row.state,
    row.pid,
    row.process_name,
    row.process_start_time ?? null,
    row.wsl_distro ?? null,
    row.wsl_start_tick ?? null,
    row.container_engine ?? null,
    row.container_id ?? null,
    row.container_name ?? null,
    row.identity ?? null,
    row.command_line ?? null,
    row.executable_path ?? null,
  ]);
}

function correlationFingerprint(row: PortRow): string {
  // Native action keys intentionally include the producer snapshot identity,
  // so they may rotate even when ownership is unchanged. Timeline ownership
  // changes are based on the public owner description, never on that opaque
  // action key.
  return JSON.stringify(
    (row.correlations ?? [])
      .map((correlation) => [
        correlation.source_app,
        correlation.target_kind,
        correlation.target_id,
        correlation.label,
        correlation.confidence,
        correlation.logs_available,
      ])
      .sort((left, right) => JSON.stringify(left).localeCompare(JSON.stringify(right))),
  );
}

function sameEndpoint(left: PortRow, right: PortRow): boolean {
  return (
    (left.source ?? "windows") === (right.source ?? "windows") &&
    canonicalProto(left.proto) === canonicalProto(right.proto) &&
    left.local_addr === right.local_addr &&
    left.port === right.port
  );
}

function compareText(left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0;
}

function compareRows(left: PortRow, right: PortRow): number {
  return (
    compareText(identityOnlyKey(left), identityOnlyKey(right)) ||
    compareText(endpointKey(left), endpointKey(right)) ||
    compareText(rowFingerprint(left), rowFingerprint(right))
  );
}

function processFavoriteKey(favorite: ProcessFavorite): string {
  return [favorite.source, identityKey(favorite.identity)].map(segment).join("|");
}

export function sameProcessFavorite(left: ProcessFavorite, right: ProcessFavorite): boolean {
  return processFavoriteKey(left) === processFavoriteKey(right);
}

export type RefreshDiffKind = "opened" | "closed" | "changed" | "owner-changed";

export interface RefreshDiff {
  kind: RefreshDiffKind;
  key: string;
  before?: PortRow;
  after?: PortRow;
}

export interface RefreshTimelineRow {
  local_addr: string;
  process_name: string | null;
  owner_labels: string[];
}

export interface RefreshTimelineEvent {
  kind: RefreshDiffKind;
  key: string;
  before?: RefreshTimelineRow;
  after?: RefreshTimelineRow;
  observed_at_ms: number;
}

function timelineRow(row: PortRow | undefined): RefreshTimelineRow | undefined {
  if (!row) return undefined;
  return {
    local_addr: row.local_addr,
    process_name: row.process_name,
    // Keep only the labels rendered by the session timeline. Native action
    // keys, target ids, process identities, commands, and executable paths
    // remain in the live observation snapshot and are never archived here.
    owner_labels: (row.correlations ?? []).map(({ label }) => label),
  };
}

/**
 * Compare only two successful snapshots. A null previous snapshot means the
 * initial load and intentionally produces no synthetic "opened" flood. Strong
 * process identities are matched without endpoint fields, so a listener that
 * moves endpoint is reported as changed. Identity-less rows fall back to an
 * endpoint key and never borrow another process identity.
 */
export function diffPortRows(previous: PortRow[] | null, next: PortRow[]): RefreshDiff[] {
  if (previous === null) return [];

  // Native adapters normally sort their snapshots, but sorting copies here
  // keeps duplicate identities deterministic for browser fixtures and future
  // adapters without mutating React-owned arrays.
  const unmatched = [...previous].sort(compareRows).map((row) => ({ row, used: false }));
  const orderedNext = [...next].sort(compareRows);
  const matches = new Array<number | null>(orderedNext.length).fill(null);

  // Reserve every exact identity+endpoint match before considering an
  // identity-only move. Without this first pass, a newly opened endpoint that
  // sorts earlier can greedily consume the previous row needed by an exact
  // match later in the snapshot and manufacture two `changed` entries.
  orderedNext.forEach((after, index) => {
    const identity = identityOnlyKey(after);
    const matchIndex = unmatched.findIndex(
      (candidate) =>
        !candidate.used &&
        identityOnlyKey(candidate.row) === identity &&
        sameEndpoint(candidate.row, after),
    );
    if (matchIndex >= 0) {
      unmatched[matchIndex].used = true;
      matches[index] = matchIndex;
    }
  });

  // A remaining strong identity can move from one endpoint to another. Rows
  // without an identity use their endpoint as identityOnlyKey, so they never
  // borrow an unrelated process through this fallback.
  orderedNext.forEach((after, index) => {
    if (matches[index] !== null) return;
    const identity = identityOnlyKey(after);
    const matchIndex = unmatched.findIndex(
      (candidate) => !candidate.used && identityOnlyKey(candidate.row) === identity,
    );
    if (matchIndex >= 0) {
      unmatched[matchIndex].used = true;
      matches[index] = matchIndex;
    }
  });

  const changes: RefreshDiff[] = [];
  orderedNext.forEach((after, index) => {
    const identity = identityOnlyKey(after);
    const matchIndex = matches[index];
    if (matchIndex === null) {
      changes.push({ kind: "opened", key: identity, after });
      return;
    }
    const before = unmatched[matchIndex].row;
    if (correlationFingerprint(before) !== correlationFingerprint(after)) {
      changes.push({ kind: "owner-changed", key: identity, before, after });
    } else if (rowFingerprint(before) !== rowFingerprint(after)) {
      changes.push({ kind: "changed", key: identity, before, after });
    }
  });
  for (const candidate of unmatched) {
    if (!candidate.used) {
      changes.push({ kind: "closed", key: identityOnlyKey(candidate.row), before: candidate.row });
    }
  }

  const order: Record<RefreshDiffKind, number> = {
    opened: 0,
    closed: 1,
    changed: 2,
    "owner-changed": 3,
  };
  return changes.sort(
    (left, right) => {
      const byKind = order[left.kind] - order[right.kind];
      if (byKind !== 0) return byKind;
      return (
        compareText(left.key, right.key) ||
        compareText(rowFingerprint(left.after ?? left.before!), rowFingerprint(right.after ?? right.before!))
      );
    },
  );
}

/**
 * Append successful refresh changes to the in-memory session timeline. The
 * caller deliberately owns the timeline ref/state; this helper has no I/O or
 * persistence and returns a fresh bounded array for React state updates.
 */
export function appendRefreshTimeline(
  timeline: RefreshTimelineEvent[],
  changes: RefreshDiff[],
  observedAtMs = Date.now(),
): RefreshTimelineEvent[] {
  const safeObservedAt = Number.isSafeInteger(observedAtMs)
    && observedAtMs >= 0
    && Number.isFinite(new Date(observedAtMs).getTime())
    ? observedAtMs
    : 0;
  const events = changes.map((change) => ({
    kind: change.kind,
    key: change.key,
    before: timelineRow(change.before),
    after: timelineRow(change.after),
    observed_at_ms: safeObservedAt,
  }));
  return [...timeline, ...events].slice(-MAX_REFRESH_TIMELINE_EVENTS);
}

export function portFavoriteFor(row: PortRow): PortFavorite {
  return {
    source: row.source ?? "windows",
    proto: canonicalProto(row.proto),
    local_addr: row.local_addr,
    port: row.port,
  };
}

export function processFavoriteFor(row: PortRow): ProcessFavorite | null {
  if (!row.identity) return null;
  return { source: row.source ?? "windows", identity: row.identity };
}

export function isPortFavorite(row: PortRow, favorites: PortFavorite[]): boolean {
  const favorite = portFavoriteFor(row);
  return favorites.some(
    (candidate) =>
      candidate.source === favorite.source &&
      canonicalProto(candidate.proto) === favorite.proto &&
      candidate.local_addr === favorite.local_addr &&
      candidate.port === favorite.port,
  );
}

export function isProcessFavorite(row: PortRow, favorites: ProcessFavorite[]): boolean {
  const favorite = processFavoriteFor(row);
  return favorite !== null && favorites.some(
    (candidate) => sameProcessFavorite(candidate, favorite),
  );
}

export function isPinnedRow(row: PortRow, preferences: PortManagerPreferences): boolean {
  return isPortFavorite(row, preferences.favorite_ports) ||
    isProcessFavorite(row, preferences.favorite_processes);
}
