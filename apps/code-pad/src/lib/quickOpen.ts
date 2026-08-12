import type { WorkspaceFile } from "../types";

export interface QuickOpenMatch {
  file: WorkspaceFile;
  score: number;
}
function normalized(value: string): string {
  return value.normalize("NFKC").toLocaleLowerCase();
}

/**
 * Scores a path using a small deterministic substring/subsequence matcher.
 * Substrings rank above fuzzy matches; contiguous and path-segment matches
 * receive a bonus.  This is intentionally local ranking, never a second file
 * walk or a project-wide grep.
 */
export function scoreQuickOpen(query: string, candidate: string): number | null {
  const needle = normalized(query.trim());
  const haystack = normalized(candidate);
  if (!needle) return 0;

  const substringAt = haystack.indexOf(needle);
  if (substringAt !== -1) {
    const boundaryBonus = substringAt === 0 || "/\\.-_".includes(haystack[substringAt - 1] ?? "") ? 80 : 0;
    return 10_000 + boundaryBonus - substringAt * 4 - (haystack.length - needle.length);
  }

  let cursor = 0;
  let first = -1;
  let previous = -1;
  let gaps = 0;
  let contiguous = 0;
  let bestContiguous = 0;
  for (const character of needle) {
    const index = haystack.indexOf(character, cursor);
    if (index === -1) return null;
    if (first === -1) first = index;
    if (previous !== -1) {
      if (index === previous + 1) {
        contiguous += 1;
        bestContiguous = Math.max(bestContiguous, contiguous);
      } else {
        gaps += index - previous - 1;
        contiguous = 0;
      }
    }
    previous = index;
    cursor = index + character.length;
  }

  const boundaryBonus = first === 0 || "/\\.-_".includes(haystack[first - 1] ?? "") ? 35 : 0;
  return 5_000 + boundaryBonus + bestContiguous * 18 - gaps * 5 - first * 2 - haystack.length;
}

export function filterQuickOpenFiles(files: WorkspaceFile[], query: string): QuickOpenMatch[] {
  return files
    .map((file) => ({ file, score: scoreQuickOpen(query, file.relativePath) }))
    .filter((match): match is QuickOpenMatch => match.score !== null)
    .sort(
      (left, right) =>
        right.score - left.score || left.file.relativePath.localeCompare(right.file.relativePath),
    );
}
