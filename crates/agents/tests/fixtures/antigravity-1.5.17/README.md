# Antigravity CLI 1.5.17 fixture

이 폴더의 파일은 모두 실제 녹화다: 2026-11-25(`idle-after-user-turn.txt`·`pasted-lines.txt`·`pasted-chars.txt`만 2026-11-26, 실제 devbox 데몬과 함께 띄운 agy), Antigravity CLI(`agy`) 1.5.17, 소유 임시 git 저장소와 전용 tmux socket(pane 200x50). 합성한 파일은 없다. 실제 화면을 한 군데 바꿔 만든 변형은 시험 코드 안에서만 만들고, 그 자리에 실제 녹화가 아니라고 적는다.

- `*.txt`: `tmux capture-pane -p` 화면. `#` 머리말이 출처 캡처 이름과 가린 것을 적고, `---` 줄 뒤가 pane 내용이다. `pasted-lines.txt`·`pasted-chars.txt`는 devbox가 읽는 그대로 `capture-pane -p -J -S -15`로 찍어 화면 위의 줄(있으면 15줄까지)과 줄 끝 빈칸이 남아 있다. agy가 큰 붙여 넣기를 입력창에 `[Pasted text #<n> +<줄 수> lines]`·`[Pasted text #<n> <글자 수> chars]`로만 보이는 화면이다.
- `hook-*.json`: hook 명령이 stdin으로 받은 payload 한 줄(`---` 줄 뒤). `hook-session-start.json`·`hook-pre-invocation.json`·`hook-stop.json`은 devbox가 등록하는 이벤트, `hook-post-invocation.json`·`hook-pre-tool-use.json`은 등록하지 않는 이벤트의 것이다.
- `transcript_full.jsonl`: hook payload의 `transcriptPath`가 가리키는 기록 파일(`---` 줄 뒤).

가린 것: 소유 경로는 `/owned/project/fixture-task`(hook 폴더는 `/owned/project/fixture-task.agy-hooks`, 그 작업 폴더의 브랜치 이름이 화면에 있으면 `agent/fixture-task`), 홈 경로는 `/owned/home`, 대화 id는 `00000000-0000-4000-8000-000000000001`로 바꿨다. 화면의 배너 둘째 줄(계정·요금제)은 `fixture-account (fixture-plan)`으로, 사용자 상태 줄에서는 토큰·사용량 한도·메모리·디스크 칸을 지웠고, 계정 상태를 알리는 두 줄짜리 알림은 글만 바꿨다. 기록 파일의 `content`·`thinking`·`error`는 `fixture`로, `tool_calls`는 이름만 남겼다. 계정·이메일·요금제·사용량 한도·홈 경로는 들어 있지 않다.
