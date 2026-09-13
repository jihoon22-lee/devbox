import { describe, expect, it } from "vitest";
import { assessBroadcastInput, MAX_BROADCAST_TARGETS, nextBroadcastTargets } from "./broadcastSafety";

describe("broadcast safety", () => {
  it("일반 단일 명령은 target 확인 없이 버퍼만 추적한다", () => {
    const typed = assessBroadcastInput("echo ok", "", 3);
    expect(typed.confirmation).toBeNull();
    expect(typed.nextPendingCommand).toBe("echo ok");
    const enter = assessBroadcastInput("\r", typed.nextPendingCommand, 3);
    expect(enter.confirmation).toBeNull();
    expect(enter.nextPendingCommand).toBe("");
  });

  it("위험 명령 Enter는 대상 수를 포함해 재확인한다", () => {
    const result = assessBroadcastInput("\r", "sudo rm -rf ./cache", 4);
    expect(result.confirmation).toContain("4개");
    expect(result.confirmation).toContain("위험");
    expect(assessBroadcastInput("\r", "sudo apt update", 4).confirmation).not.toBeNull();
    expect(assessBroadcastInput("\r", "rm one.txt", 4).confirmation).not.toBeNull();
    expect(assessBroadcastInput("\r", "rm -fr ./cache", 4).confirmation).not.toBeNull();
    expect(assessBroadcastInput("\r", "rm --recursive ./cache", 4).confirmation).not.toBeNull();
    expect(assessBroadcastInput("\r", "rm file-not-recursive.txt", 4).confirmation).not.toBeNull();
    expect(assessBroadcastInput("\r", "cat < input.txt", 4).confirmation).not.toBeNull();
    expect(assessBroadcastInput("\r", "echo ok > shared.txt", 4).confirmation).not.toBeNull();
    expect(assessBroadcastInput("\r", "echo ok>shared.txt", 4).confirmation).not.toBeNull();
    expect(assessBroadcastInput("\r", "cat<input.txt", 4).confirmation).not.toBeNull();
  });

  it("여러 줄 paste는 명령 내용 없이 대상 수와 실행 위험을 알린다", () => {
    const result = assessBroadcastInput("echo one\necho two\n", "", 2);
    expect(result.confirmation).toContain("2개");
    expect(result.confirmation).not.toContain("echo one");
  });

  it("backspace를 반영하고 버퍼를 4096자로 제한한다", () => {
    expect(assessBroadcastInput("\u007f!", "abc", 2).nextPendingCommand).toBe("ab!");
    expect(assessBroadcastInput("x".repeat(5000), "", 2).nextPendingCommand).toHaveLength(4096);
  });

  it("native broadcast 대상 상한을 넘는 선택을 거부한다", () => {
    const full = new Set(Array.from({ length: MAX_BROADCAST_TARGETS }, (_, index) => `s${index}`));
    expect(nextBroadcastTargets(full, "s-overflow", true)).toBeNull();
    expect(nextBroadcastTargets(full, "s0", true)?.size).toBe(MAX_BROADCAST_TARGETS);
    expect(nextBroadcastTargets(full, "s0", false)?.has("s0")).toBe(false);
  });
});
