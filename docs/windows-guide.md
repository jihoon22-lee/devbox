# Windows 11에서 devbox 앱 사용 가이드

이 가이드는 **Windows 11 PC(예: 회사 PC)에서 12개 앱을 직접 빌드하고 실행**하는 방법을 설명한다.
개발은 WSL에서 했지만, 앱 자체는 Windows 데스크톱 앱(Tauri)이므로 Windows PC에서 `.exe`로 빌드하면 그대로 쓸 수 있다.

> 저장소: `https://github.com/jihoon22-lee/devbox` (공개 저장소)
> 앱별 산출물: `PortManager.exe` `DevToolbox.exe` `WSLDashboard.exe` `ApiPlayground.exe`
> `ActivityTimeline.exe` `EverythingPlus.exe` `Knowledge.exe` `LifeLog.exe`
> `WSLDesktop.exe` `DevboxManager.exe` `CodePad.exe` `RunManager.exe`

---

## 0. (권장) 빌드 환경 없이 Releases에서 바로 받아 쓰기

**한 번 빌드해 두면, 이후에는 회사 PC에 아무것도 설치하지 않고 실행 파일만 내려받아 쓸 수 있다.**

1. GitHub 저장소의 **Releases** 페이지로 이동:
   `https://github.com/jihoon22-lee/devbox/releases`
2. 최신 릴리스에서 원하는 앱의 **`*-setup.exe`** (또는 실행 파일 `*.exe`)를 다운로드.
3. 더블클릭해 실행. **WebView2 런타임(Windows 11 기본 포함)만 있으면 동작**한다.
   - Rust·Node·MSVC 같은 빌드 도구는 **필요 없다** (이미 빌드된 실행 파일이기 때문).

> 빌드를 새로 하고 싶을 때(GitHub Actions가 대신 빌드):
> 1. 루트 `CHANGELOG.md`에 새 버전 섹션(`## [vX.Y.Z] - 날짜`)으로 변경점 기록
> 2. **방법 1 (태그로 배포, 권장)**: WSL/로컬에서 `git tag v0.1.1 && git push origin v0.1.1`
>    - **방법 2 (수동)**: GitHub → Actions 탭 → **Release** → **Run workflow** → 버전 입력(예: `v0.1.1`)
> 3. 그러면 Windows CI가 12개 앱을 빌드해 **릴리스 노트는 CHANGELOG의 해당 버전 내용으로** 새 릴리스를 만든다.
>    버전(tag)은 **매번 새로** 써야 한다(기존 tag 재사용 불가).

> 참고: 개인 빌드라 코드 서명이 없어 SmartScreen 경고가 뜨면 `추가 정보 → 실행`을 누르면 된다.

아래부터는 **직접 빌드하고 싶을 때**의 상세 절차다.

---

## 0. 준비물 요약

| 항목 | 필요 이유 | 확인 방법 |
|---|---|---|
| Windows 11 (x64) | 대상 OS | `설정 → 시스템 → 정보` |
| WebView2 런타임 | Tauri 앱의 웹엔진 (Win11 기본 포함) | 보통 설치돼 있음 (아래 3.4 참고) |
| Git | 소스 내려받기 | `git --version` |
| Node.js LTS | 프론트 빌드 | `node --version` |
| pnpm | 워크스페이스 패키지 매니저 | `pnpm --version` |
| Rust (MSVC) | Rust 백엔드 컴파일 | `rustc --version`, `cargo --version` |
| MSVC C++ Build Tools | Rust 링커(link.exe) | `winget list` 또는 VS Installer |

---

## 1. 터미널 준비

- **PowerShell**을 엽니다 (Win+X → Windows Terminal(PowerShell) 또는 시작 메뉴에서 PowerShell).
- 이후의 모든 명령은 PowerShell에서 실행합니다.
- **관리자 권한은 권장 사항**: `winget install`은 관리자 권한이 편합니다.
  - 시작 메뉴에서 "PowerShell" 우클릭 → **관리자 권한으로 실행**

---

## 2. Git 설치

```powershell
winget install --id Git.Git -e --source winget
```

설치 후 **새 터미널**을 열어 확인:

```powershell
git --version
```

---

## 3. Rust + MSVC 빌드 도구 설치 (가장 중요)

Tauri는 Rust 코드를 MSVC 컴파일러로 빌드한다. **빌드 도구 먼저, Rust 그다음** 순서로 설치한다.

### 3.1 MSVC C++ Build Tools (link.exe 포함)

```powershell
winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

- 수 GB 다운로드 + 몇 분 소요. 콘솔이 끝날 때까지 기다립니다.
- 완료 확인 (새 터미널):
  ```powershell
  Get-ChildItem "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC" | Select-Object Name
  ```
  버전 폴더가 보이면 성공.

### 3.2 Rust 툴체인 (MSVC 기본 툴체인)

```powershell
winget install --id Rustlang.Rustup -e --source winget
```

설치 후 **새 터미널**에서 확인 (PATH 반영을 위해 새 창 필요):

```powershell
rustc --version   # 예: rustc 1.8x.x
cargo --version
rustup show       # default host: x86_64-pc-windows-msvc 확인
```

> `rustc`가 안 보이면: `C:\Users\<you>\.cargo\bin`을 PATH에 추가하거나 재부팅.

---

## 4. Node.js + pnpm 설치

### 4.1 Node.js LTS

```powershell
winget install --id OpenJS.NodeJS.LTS -e --source winget
```

새 터미널에서:

```powershell
node --version
npm --version
```

### 4.2 pnpm (corepack 권장)

Node 16.13+ 에 포함된 corepack으로 활성화:

```powershell
corepack enable pnpm
pnpm --version   # 예: 9.15.9 (저장소 packageManager: pnpm@9.0.0)
```

> corepack이 안 되면 대안: `npm install -g pnpm`

---

## 5. WebView2 런타임 확인

Windows 11에는 기본 포함되어 있지만, 확인:

```powershell
Get-ChildItem "C:\Program Files (x86)\Microsoft\EdgeWebView\Application" -ErrorAction SilentlyContinue | Select-Object Name
```

버전 폴더가 보이면 OK. 없으면:

```powershell
winget install --id Microsoft.EdgeWebView2Runtime -e
```

---

## 6. 소스 받기

```powershell
cd C:\   # 원하는 위치
git clone https://github.com/jihoon22-lee/devbox.git
cd devbox
pnpm install
```

- `pnpm install`은 12개 앱의 의존성을 한 번에 설치한다 (몇 분).
- `node_modules`는 워크스페이스 루트에 통합 관리된다.

---

## 7. 앱 빌드 (`.exe` 만들기)

각 앱 디렉터리에서 `pnpm tauri build`를 실행한다. **첫 빌드는 의존성 컴파일 때문에 5~10분** 걸리고, 이후에는 빠르다.

### 7.1 한 앱만 빌드

```powershell
cd C:\devbox\apps\port-manager
pnpm tauri build
```

### 7.2 전부 한 번에 빌드 (권장)

```powershell
cd C:\devbox
$apps = "port-manager","developer-toolbox","wsl-dashboard","api-playground","activity-timeline","everything-plus","knowledge-base","life-log","wsl-desktop","devbox-manager","code-pad","run-manager"
foreach ($a in $apps) {
  Write-Host "===== BUILDING $a =====" -ForegroundColor Cyan
  Push-Location "apps\$a"
  pnpm tauri build
  Pop-Location
}
```

### 7.3 산출물 위치

devbox는 **Cargo workspace**이므로, 어떤 앱에서 빌드하든 산출물이 **저장소 루트의 `target\`** 아래에 모인다:
```
C:\devbox\target\release\<ProductName>.exe                                        ← 실행 파일 (단일)
C:\devbox\target\release\bundle\nsis\<ProductName>_0.3.0_x64-setup.exe             ← 설치 패키지
```

ProductName 매핑:

| 앱 디렉터리 | ProductName |
|---|---|
| port-manager | PortManager |
| developer-toolbox | DevToolbox |
| wsl-dashboard | WSLDashboard |
| api-playground | ApiPlayground |
| activity-timeline | ActivityTimeline |
| everything-plus | EverythingPlus |
| knowledge-base | Knowledge |
| life-log | LifeLog |
| wsl-desktop | WSLDesktop |
| devbox-manager | DevboxManager |
| code-pad | CodePad |
| run-manager | RunManager |

---

## 8. 실행

- **방법 A (권장)**: `bundle\nsis`의 `*-setup.exe`로 설치 → 시작 메뉴에서 실행
- **방법 B**: `target\release\<ProductName>.exe`를 바로 더블클릭 (설치 없이 실행)

SmartScreen 경고("인식할 수 없는 앱")가 뜨면:
1. `추가 정보` 클릭 → `실행` 클릭
   (코드 서명이 없어서 나오는 정상 경고. 개인 빌드이므로 안전)

---

## 9. 앱별 사용 메모

| 앱 | 사용 팁 |
|---|---|
| **PortManager** | 포트/Kill/열기. 시스템 프로세스 Kill이 실패하면 **관리자 권한으로 실행**. |
| **DevToolbox** | 좌측 메뉴에서 도구 선택. Hash/UUID/Regex/Diff는 Rust 연동. |
| **WSLDashboard** | WSL2 필요: `wsl --install` 후 재부팅. Docker 컨테이너 관리엔 Docker Desktop 필요. |
| **ApiPlayground** | URL 입력 → Send. Rust가 직접 요청하므로 CORS 없음. History는 자동 저장. |
| **ActivityTimeline** | 실행하면 추적 시작. 창을 닫아도 **트레이 아이콘**으로 계속 기록. 종료는 트레이 → Quit. |
| **EverythingPlus** | 첫 실행 시 `+`로 검색 루트 추가(예: `C:\`, `D:\`) → 자동 인덱싱. |
| **Knowledge** | 기본 저장 위치: `Documents\Knowledge`. 우측에서 작성, Ctrl+S 저장. Daily note 버튼으로 오늘 메모. |
| **LifeLog** | 설정 탭에서 **활동 데이터 소스**(activity-timeline의 data.db)와 **git 프로젝트 경로**를 등록해야 값이 채워짐. |
| **WSLDesktop** | 임베디드 WSL 터미널. WSL2 필요: `wsl --install` 후 재부팅. |
| **DevboxManager** | devbox 앱 설치·업데이트·실행을 한 곳에서 관리. |
| **CodePad** | CodeMirror 6 기반 코드 에디터. `언어 서버` 패널에서 LSP 서버 설치·활성화 후 진단·이름 변경·포맷 사용. |
| **RunManager** | 작업(cron)·서비스 정의, 실행 이력·로그 tail. 서비스는 시작/정지/재시작과 헬스체크·재시작 정책 지원. |

---

## 10. 데이터 위치 (앱들이 저장하는 곳)

Tauri의 `app_local_data_dir()`은 번들 identifier 기준 폴더를 사용한다.

```
%LOCALAPPDATA%\com.workbench.activitytimeline\data.db   ← 활동 기록
%LOCALAPPDATA%\com.workbench.everythingplus\data.db     ← 파일 인덱스
%LOCALAPPDATA%\com.workbench.knowledgebase\data.db      ← 문서 인덱스
%LOCALAPPDATA%\com.workbench.lifelog\data.db            ← life-log 설정
```

- **집 ↔ 회사 데이터 공유**: 위 폴더를 통째로 복사하면 기록/인덱스가 이전된다.
- Knowledge 문서 파일 자체는 `Documents\Knowledge`에 있으므로, 이 폴더만 복사해도 됨.
- Life Log는 Settings 탭에서 활동 데이터 소스 경로를 변경할 수 있다 (기본값은 activity-timeline 경로).

---

## 11. 자주 겪는 문제

| 증상 | 해결 |
|---|---|
| `link.exe` 또는 `LINK : fatal error` | MSVC Build Tools의 "C++를 사용한 데스크톱 개발" 워크로드가 빠짐 → 3.1 재실행 |
| `'pnpm' is not recognized` | corepack/npm -g 설치 후 새 터미널. `corepack enable pnpm` |
| `rustc` 없음 | 새 터미널 열기. 안 되면 `C:\Users\<you>\.cargo\bin` PATH 추가 후 재시작 |
| `WebView2` 관련 런타임 오류 | 5번 참고해 런타임 설치 |
| 빌드가 `tauri.conf.json` 못 찾음 | `apps\<앱>` 디렉터리에서 실행했는지 확인 (`pwd`) |
| SmartScreen 경고 | `추가 정보 → 실행` (서명 없는 개인 빌드) |
| 회사 네트워크가 GitHub 차단 | IT에 `github.com` 접근 허용 요청 (HTTPS 443) |
| 빌드가 느림 | 첫 빌드만 그럼. 이후 증분 빌드는 빠름 |

---

## 12. (선택) 개발 모드로 수정하며 쓰기

```powershell
cd C:\devbox\apps\port-manager
pnpm tauri dev
```

코드 수정 → 저장하면 자동 새로고침되는 개발 창이 뜬다. 원상태로 되돌리려면 `git restore` 후 다시 빌드.

---

## 한눈에 보는 빠른 시작 (모든 명령 순서)

```powershell
# 1. 도구 설치 (관리자 PowerShell)
winget install Git.Git
winget install Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
winget install Rustlang.Rustup
winget install OpenJS.NodeJS.LTS
corepack enable pnpm

# 2. 새 터미널에서
rustc --version && cargo --version && node --version && pnpm --version

# 3. 소스 & 빌드
git clone https://github.com/jihoon22-lee/devbox.git
cd devbox
pnpm install
cd apps\port-manager
pnpm tauri build

# 4. 실행
.\target\release\bundle\nsis\PortManager_0.3.0_x64-setup.exe
```
