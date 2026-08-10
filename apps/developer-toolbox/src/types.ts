export interface RegexMatch {
  start: number;
  end: number;
  text: string;
}

export interface DiffHunk {
  kind: number;
  old_start: number;
  old_end: number;
  new_start: number;
  new_end: number;
}
