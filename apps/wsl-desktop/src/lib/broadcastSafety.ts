const MAX_PENDING_COMMAND = 4096;

export interface BroadcastAssessment {
  confirmation: string | null;
  nextPendingCommand: string;
}

const DANGEROUS_COMMAND = /(?:^|[;&|]\s*)(?:sudo\s+)?(?:rm\s+(?:[^\s\r\n]+\s+)*-[^\s\r\n]*r|shutdown|reboot|poweroff|mkfs(?:\.|\s)|dd\s+[^\r\n]*\bof=|docker\s+system\s+prune|kubectl\s+delete|drop\s+(?:database|table)|truncate\s+table|git\s+clean\s+-[^\r\n]*f)/iu;

function updatePending(previous: string, data: string): string {
  let pending = previous;
  for (const character of data) {
    if (character === "\r" || character === "\n") pending = "";
    else if (character === "\u007f" || character === "\b") pending = pending.slice(0, -1);
    else if (character >= " " && character !== "\u007f") pending = (pending + character).slice(-MAX_PENDING_COMMAND);
  }
  return pending;
}

export function assessBroadcastInput(
  data: string,
  pendingCommand: string,
  targetCount: number,
): BroadcastAssessment {
  const logicalLines = data.split(/\r\n|\r|\n/u);
  const containsLineBreak = logicalLines.length > 1;
  const multiline = logicalLines.length > 2
    || (containsLineBreak && logicalLines.some((line, index) => index > 0 && line.length > 0));
  const submitted = containsLineBreak ? `${pendingCommand}${logicalLines[0] ?? ""}` : "";
  let confirmation: string | null = null;
  if (multiline) {
    confirmation = `${targetCount}개 터미널에 여러 줄 입력을 동시에 보낼까요? 각 줄이 명령으로 실행될 수 있습니다.`;
  } else if (submitted && DANGEROUS_COMMAND.test(submitted)) {
    confirmation = `${targetCount}개 터미널에 위험할 수 있는 명령을 동시에 보낼까요?`;
  }
  return { confirmation, nextPendingCommand: updatePending(pendingCommand, data) };
}
