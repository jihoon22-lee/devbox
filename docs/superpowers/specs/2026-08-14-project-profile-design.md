# ProjectProfile 설계 — 프로젝트 단일 identity

- 상태: 제안(Proposal) 및 설계 기록 — PR 28의 구현은 v0.5.0 stable에 반영됐고 v0.5.1에서도
  유지한다. 이 maintenance release는 proposal의 역사적 범위를 재작성하지 않는다.
- 작성일: 2026-08-14
- 근거: `docs/product-opportunities.md` §10.2, §15.2 (Workbench)
- 선행: PR 14 (`crates/wsl`의 `canonical_project_key`)

## 1. 문제

Windows path, WSL path, Git root, worktree가 앱마다 별도 문자열이면 같은 프로젝트를
서로 다르게 인식한다. 이미 두 벌이 존재한다.

| 앱 | 저장 위치 |
|---|---|
| wsl-desktop | localStorage `wsld-projects` (`src/lib/projectPaths.ts`) |
| life-log | SQLite settings `projects` |

ProjectProfile은 세 번째 저장소가 아니라 **이 둘을 흡수**하는 것이다.

## 2. 목표

- 한 프로젝트를 Windows·WSL 어느 쪽에서 등록해도 하나로 식별한다 (canonical identity)
- Workbench가 단일 writer가 되어 다른 앱은 project ID와 필요한 context만 전달받는다
- 여러 앱이 같은 공유 JSON을 동시에 수정하지 않는다 (단일 writer 원칙)

## 3. 스키마

```ts
interface ProjectProfile {
  id: string;                  // UUID v4
  name: string;
  windowsPath: string | null;  // E:\projects\devbox
  wsl: { distro: string; path: string } | null;  // /mnt/e/projects/devbox
  gitRoot: string | null;      // 저장소 루트 (windows 또는 wsl 표기)
  preferredEditor: string | null;   // code-pad | null
  terminalProfileId: string | null; // wsl-desktop 탭/레이아웃 preset id
  runManagerJobIds: string[];
  runManagerServiceIds: string[];
  expectedPorts: number[];
}
```

### canonical identity

`crates/wsl::canonical_project_key`가 유일한 규칙이다.

- Windows 경로 `E:\projects\devbox` → `win:e:/projects/devbox`
- WSL `/mnt/e/projects/devbox` → 같은 `win:e:/projects/devbox` 키
- `/mnt/` 밖 WSL 경로 → `wsl:<distro>:<path>` (distro 포함)

프로젝트 등록 시 두 경로 중 하나라도 주어지면 키를 계산하고, DB 유니크 인덱스로
같은 프로젝트 중복 등록을 막는다. 두 경로가 모두 주어지면 둘을 대조해 어긋나면 오류.

## 4. 저장 위치와 형식

- **Workbench가 단일 writer**다 (Workbench 착수 전까지는 설계만 고정).
- 저장: Workbench의 app data `%LOCALAPPDATA%\com.devbox.workbench\project-profiles.json`
- 파일 하나를 통째로 read-modify-write (임시 파일 + rename 원자 교체).
- 라이선스·충돌 걱정 없이 파일 기반으로 충분 (프로젝트 수는 수십~수백 수준).

### 다른 앱이 읽는 방식

- Workbench가 전용 command/이벤트로 전달하거나, integration snapshot 계약(§10.1)과
  같은 방식으로 read-only로 노출한다. **다른 앱이 파일을 직접 수정하지 않는다.**

## 5. 스키마 버전 정책

- 파일 최상단 `schemaVersion: 1`
- 스키마가 바뀌면 version을 올리고, Workbench가 마이그레이션을 수행한다
- 다른 앱은 자신이 아는 version만 소비한다 (모르면 해당 필드를 무시)

## 6. 기존 두 저장소 흡수

| 원본 | 위치 | 마이그레이션 |
|---|---|---|
| wsl-desktop `wsld-projects` | localStorage | Workbench 첫 실행 시 읽어 `windowsPath` 프로필로 등록 |
| life-log settings `projects` | SQLite | Workbench가 life-log settings의 프로젝트 경로를 읽어 등록 |

마이그레이션 후 원본은 남겨둔다 (Workbench가 충분히 검증된 뒤 제거). 중복은
canonical key로 병합한다.

## 7. 다른 앱에 전달하는 최소 context

- wsl-desktop → `wsl.desktop` 열기: `{ profileId, distro, path, terminalProfileId }`
- code-pad → workspace 열기: `{ profileId, path }`
- port-manager → expected port 확인: `{ profileId, expectedPorts }`
- run-manager → 서비스/잡: `{ profileId, serviceIds, jobIds }`

## 8. 완료 조건

- canonical identity 규칙이 `crates/wsl`의 함수를 사용한다
- 단일 writer 원칙이 명시됐다
- 스키마가 위 TS 타입/JSON으로 문서화됐다
- 기존 두 저장소의 마이그레이션 경로가 있다
