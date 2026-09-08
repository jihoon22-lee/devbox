import type { WorkspaceFile } from "../types";

export interface QuickOpenMatch {
  file: WorkspaceFile;
  score: number;
}

export interface QuickOpenDirectory {
  /** Workspace-relative directory path, using `/` on every platform. */
  path: string;
  /** The final directory name displayed in the tree heading. */
  name: string;
  directories: QuickOpenDirectory[];
  files: QuickOpenMatch[];
}

export interface QuickOpenTree {
  /** Files directly below the workspace root. */
  files: QuickOpenMatch[];
  /** Directories below the workspace root, recursively grouped by path. */
  directories: QuickOpenDirectory[];
}

export interface QuickOpenPathParts {
  directory: string;
  name: string;
}

function normalized(value: string): string {
  return value.normalize("NFKC").toLowerCase();
}

function compareText(left: string, right: string): number {
  if (left < right) return -1;
  if (left > right) return 1;
  return 0;
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
        right.score - left.score || compareText(left.file.relativePath, right.file.relativePath),
    );
}

/**
 * Splits a workspace-relative path without hiding the parent context. The
 * backend currently emits `/`, but accepting `\\` here keeps the view safe for
 * fixtures and future Windows-only callers.
 */
export function splitQuickOpenPath(path: string): QuickOpenPathParts {
  const segments = path.split(/[\\/]+/).filter(Boolean);
  if (segments.length === 0) return { directory: "", name: path };
  return {
    directory: segments.slice(0, -1).join("/"),
    name: segments[segments.length - 1] ?? path,
  };
}

function compareMatches(left: QuickOpenMatch, right: QuickOpenMatch): number {
  return right.score - left.score || compareText(left.file.relativePath, right.file.relativePath);
}

function compareDirectories(left: QuickOpenDirectory, right: QuickOpenDirectory): number {
  const leftScore = bestDirectoryScore(left);
  const rightScore = bestDirectoryScore(right);
  return rightScore - leftScore || compareText(left.path, right.path);
}

function bestDirectoryScore(directory: QuickOpenDirectory): number {
  return Math.max(
    directory.files[0]?.score ?? -Infinity,
    ...directory.directories.map(bestDirectoryScore),
  );
}

/**
 * Builds a filtered directory tree without changing the fuzzy ranking inside a
 * directory. Only matched files are inserted, so an unrelated directory never
 * appears as search noise. The returned tree is presentation data; it does not
 * trigger another filesystem walk or alter the path used to open a file.
 */
export function groupQuickOpenMatches(matches: QuickOpenMatch[]): QuickOpenTree {
  const root: QuickOpenDirectory = {
    path: "",
    name: "작업 폴더",
    directories: [],
    files: [],
  };
  const directories = new Map<string, QuickOpenDirectory>();

  for (const match of matches) {
    const parts = splitQuickOpenPath(match.file.relativePath);
    if (!parts.directory) {
      root.files.push(match);
      continue;
    }

    let parent = root;
    const segments = parts.directory.split("/").filter(Boolean);
    for (const [index, segment] of segments.entries()) {
      const path = segments.slice(0, index + 1).join("/");
      let directory = directories.get(path);
      if (!directory) {
        directory = { path, name: segment, directories: [], files: [] };
        directories.set(path, directory);
        parent.directories.push(directory);
      }
      parent = directory;
    }
    parent.files.push(match);
  }

  const sortDirectory = (directory: QuickOpenDirectory) => {
    directory.files.sort(compareMatches);
    directory.directories.sort(compareDirectories);
    directory.directories.forEach(sortDirectory);
  };
  root.files.sort(compareMatches);
  root.directories.sort(compareDirectories);
  root.directories.forEach(sortDirectory);

  return { files: root.files, directories: root.directories };
}

/** Returns matches in the same order as the grouped tree is rendered. */
export function flattenQuickOpenTree(tree: QuickOpenTree): QuickOpenMatch[] {
  const flattened = [...tree.files];
  const appendDirectories = (directories: QuickOpenDirectory[]) => {
    for (const directory of directories) {
      flattened.push(...directory.files);
      appendDirectories(directory.directories);
    }
  };
  appendDirectories(tree.directories);
  return flattened;
}
