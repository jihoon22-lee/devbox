// ParentProcessId is only a PID and can refer to a newer, unrelated process.
// Keep UTC 100 ns precision in identity checks and reject backward parent edges.
// https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-process
export function createdUtcExpression(subject) {
  if (!/^\$(?:_|[A-Za-z][A-Za-z0-9]*)$/u.test(subject)) throw new Error("invalid fixed PowerShell subject");
  return `${subject}.CreationDate.ToUniversalTime().ToString('o',[Globalization.CultureInfo]::InvariantCulture)`;
}
function validCreated(value) {
  return typeof value === "string" && /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{7}Z$/u.test(value) && Number.isFinite(Date.parse(value));
}
function identityKey(item) { return `${item.Pid}:${item.Created}:${item.Name}:${item.Path}`; }
function trace(root, candidates, allowUnknownRoot) {
  if (!root || !Number.isSafeInteger(root.Pid) || root.Pid <= 0) return [];
  if (!validCreated(root.Created) && !allowUnknownRoot) return [];
  const owned = new Map([[root.Pid, validCreated(root.Created) ? root.Created : null]]);
  let changed = true;
  while (changed) {
    changed = false;
    for (const item of candidates) {
      if (owned.has(item.Pid) || !owned.has(item.ParentPid)) continue;
      if (!validCreated(item.Created)) {
        const error = new Error("Windows process creation identity was invalid"); error.name = "AcceptanceError"; throw error;
      }
      const parentCreated = owned.get(item.ParentPid);
      // Equal times are allowed: CIM has 100 ns serialization but the OS clock
      // can record a parent and child in the same tick. Preserve every digit.
      if (parentCreated !== null && item.Created < parentCreated) continue;
      owned.set(item.Pid, item.Created); changed = true;
    }
  }
  return candidates.filter(item => item.Pid !== root.Pid && owned.has(item.Pid));
}
export function ownedDescendantsFromSnapshot(rootIdentity, all) {
  if (!rootIdentity) return [];
  const root = all.find(item => identityKey(item) === identityKey(rootIdentity));
  return root ? trace(root, all, false) : [];
}
// Missing root identities can only produce uncertainty, never kill authority.
export function potentialDescendantsFromSnapshots(rootIdentity, all, baseline) {
  const before = new Set(baseline.map(identityKey));
  return trace(rootIdentity, all.filter(item => !before.has(identityKey(item))), true);
}
