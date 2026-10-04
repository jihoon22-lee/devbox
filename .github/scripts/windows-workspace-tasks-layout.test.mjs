import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

const chrome = "/usr/bin/google-chrome";
// Mirrors TaskSidebar, JobSection, JobEditor and RunHistory structural classes.
// Synthetic labels exercise dense actions without native runtime or user data.
const button = (name) => `<button class="button-secondary">${name}</button>`;
const field = (name) =>
  `<label class="field"><span>${name}</span><input aria-label="${name}" value="synthetic"></label>`;
const views = {
  jobs: `<section class="jobs-section"><div class="section-toolbar"><p class="subtitle">예약된 작업을 활성화하고 실행 정책을 관리합니다.</p>${button("+ 새 작업")}${button("정의와 task 가져오기")}</div><div class="job-list"><article class="job-card"><div class="job-card-main"><div class="job-title-row"><h3>합성 작업 이름</h3><span class="job-state">활성화됨</span></div><code>synthetic-command --argument synthetic</code><div class="job-meta">Windows · 수동 실행</div></div><div class="job-actions">${["지금 실행", "비활성화", "편집", "삭제"].map(button).join("")}</div></article></div></section>`,
  editor: `<form class="editor-layout"><div class="editor-main"><header class="editor-header"><div><h3>새 작업</h3><p class="subtitle">예약 실행에 필요한 정의를 저장합니다.</p></div><div class="editor-actions">${button("취소")}${button("작업 저장")}</div></header><section class="form-card"><div class="form-grid">${field("작업 이름")}${field("cron 일정")}</div><fieldset class="form-section"><legend>실행 대상</legend><div class="target-options"><label class="target-option"><input type="radio">Windows 명령</label><label class="target-option"><input type="radio">WSL 명령</label></div></fieldset><section class="form-section"><div class="section-heading"><h3>환경변수</h3>${button("환경변수 추가")}</div><div class="environment-row"><input aria-label="환경변수 이름"><input aria-label="환경변수 값"><button class="icon-button">×</button></div></section></section></div><aside class="preview-card"><h3>다음 실행 시각</h3></aside></form>`,
  history: `<section class="history-section"><div class="section-toolbar"><h3>실행 기록</h3><div class="history-actions">${button("새로 고침")}${button("선택 기록 삭제")}</div></div><div class="history-filters">${["작업", "결과", "시작 시각", "종료 시각"].map(field).join("")}</div><div class="history-layout"><div class="run-list"><button class="run-row"><span class="run-status">성공</span><span><strong>합성 작업</strong><small>synthetic run</small></span><span><small>2026-10-04</small></span></button></div><div class="run-detail"><header class="run-detail-heading"><div><h3>실행 상세</h3><span class="run-status">성공</span></div><div class="stream-tabs">${button("stdout")}${button("stderr")}</div></header><div class="log-panel"><div class="log-search">${["로그 검색", "스트림", "시작", "끝"].map(field).join("")}${button("검색")}${button("초기화")}</div><pre>synthetic log</pre></div></div></div></section>`,
};

test("Tasks navigation, cards, editor and log controls fit the 655px zoomed viewport", {
  skip: !existsSync(chrome) && "isolated Chromium fixture requires local Chrome",
}, async () => {
  const directory = await mkdtemp(path.join(tmpdir(), "devbox-tasks-layout-"));
  try {
    const evidenceDirectory = process.env.DEVBOX_TASKS_LAYOUT_EVIDENCE_DIR;
    if (evidenceDirectory) await mkdir(evidenceDirectory, { recursive: true });
    const css = (
      await readFile(new URL("../../packages/workspace-features/src/tasks/App.css", import.meta.url), "utf8")
    ).replace(/^@import.*;$/gm, "");
    for (const [view, content] of Object.entries(views)) {
      const html = `<meta charset="utf-8"><style>body{margin:0}${css}</style><div class="workspace-feature-tasks"><main class="app-shell"><aside class="sidebar"><div class="brand-mark">RM</div><div><h1>Run Manager</h1><p>작업과 서비스를 한곳에서 관리합니다.</p></div><nav aria-label="주요 화면">${["작업", "서비스", "실행 기록"].map((name) => `<button class="nav-item">${name}</button>`).join("")}</nav><div class="sidebar-actions"><p>로그인 자동 시작은 Control Center의 환경 설정에서 변경합니다.</p>${button("안전하게 종료")}</div></aside><section class="content"><header><div><span class="eyebrow">로컬 스케줄러</span><h2>작업</h2></div><span class="status">스케줄러 준비됨</span></header>${content}</section></main></div><pre id="result" hidden></pre><script>const root=document.querySelector('.workspace-feature-tasks');result.textContent=JSON.stringify({view:${JSON.stringify(view)},viewport:innerWidth,available:document.documentElement.clientWidth,height:innerHeight,width:root.getBoundingClientRect().width,scrollWidth:root.scrollWidth,documentWidth:document.documentElement.scrollWidth,controls:[...root.querySelectorAll('button,input')].map(el=>{const r=el.getBoundingClientRect();return{left:r.left,right:r.right,width:r.width,text:el.textContent||el.getAttribute('aria-label')}})});</script>`;
      const file = path.join(directory, `${view}.html`);
      await writeFile(file, html);
      const dom = execFileSync(
        chrome,
        [
          "--headless=new",
          "--no-sandbox",
          "--window-size=655,600",
          `--user-data-dir=${directory}/profile`,
          "--dump-dom",
          ...(evidenceDirectory ? [`--screenshot=${path.join(evidenceDirectory, `${view}.png`)}`] : []),
          `file://${file}`,
        ],
        { timeout: 15000, maxBuffer: 1024 * 1024, stdio: ["ignore", "pipe", "ignore"] },
      ).toString();
      const measurement = JSON.parse(dom.match(/<pre id="result"[^>]*>(.*?)<\/pre>/s)[1]);
      assert.equal(measurement.viewport, 655);
      assert.equal(measurement.width, measurement.available, JSON.stringify(measurement));
      assert.ok(
        measurement.scrollWidth <= measurement.available && measurement.documentWidth <= measurement.available,
        JSON.stringify(measurement),
      );
      console.log(
        JSON.stringify({
          view,
          viewport: measurement.viewport,
          height: measurement.height,
          availableWidth: measurement.available,
          rootWidth: measurement.width,
          scrollWidth: measurement.scrollWidth,
          controls: measurement.controls.length,
        }),
      );
      for (const control of measurement.controls)
        assert.ok(
          control.left >= 0 && control.right <= measurement.available && control.width > 0,
          JSON.stringify({ view, control }),
        );
    }
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
