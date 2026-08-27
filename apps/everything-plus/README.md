# everything-plus — Everything+ 개인 검색기

로컬 파일을 이름·내용으로 초고속 검색하는 앱. Rust 인덱싱·FTS5·파일 감시를 다루는 성능 중심 프로젝트.
산출물: `EverythingPlus.exe` (`apps/everything-plus`).

## 주요 기능

- **파일명 검색** — FTS5 인덱스, 파일명·경로 부분 일치, 밀리초 응답
- **내용 검색** — 텍스트 파일(TXT/MD/JSON/CSV/소스코드), PDF text, XLS/XLSX/ODS 셀 값 내용 FTS5, 스니펫 하이라이트
- **정규식 모드** — regex 검색
- **인덱스 관리** — 검색 루트(드라이브/폴더) 추가·제외 규칙, re-index 진행률 표시, 파일 감시로 최신성 유지
- **결과 작업** — 열기·폴더에서 보기·경로/파일명 복사와 설치된 `path` capability 앱으로 열기 context menu
- **앱 간 검색** — catalog `Query`를 cold start와 실행 중 재호출에서 수신해 name/non-regex 검색으로 즉시 연결

## 내용 인덱스 경계

`index content`를 켠 검색 루트만 내용 인덱싱 대상이 된다. Everything+는 파일을 전부
읽지 않고 `src-tauri/src/core/content.rs`의 명시적 source/Markdown/plain-text 확장자와
PDF, Excel `.xls`/`.xlsx`, OpenDocument `.ods`, `README`, `LICENSE`, `Dockerfile`, `.gitignore`
같은 소수의 이름만 선택한다. plain-text/PDF/XLS/XLSX/ODS extractor 버전은 각각
`text-v1`/`pdf-v1`/`xls-v1`/`xlsx-v1`/`ods-v1`이다. `meta`의 독립적인 format marker가
첫 설치와 각 parser 버전 전환을 감지한다. DOCX, OCR, semantic search는 이 기능에
포함하지 않는다.

- UTF-8(선택적 BOM)과 UTF-16 little/big endian(BOM, 안정적으로 식별 가능한 BOM 없는
  텍스트)을 strict decode한다. invalid UTF-8/UTF-16, binary/NUL 데이터는 검색 hit가 되지
  않는 `unsupported_encoding` 상태로 남는다.
- 파일 하나는 최대 20 MiB, 보관 text는 최대 2,000,000 Unicode scalar characters,
  후보 하나의 cooperative processing budget은 10초다. PDF는 parser가 inflate하는
  page/object stream을 16 MiB, parsed indirect object를 100,000개, page를 10,000개로
  제한한다. object/page 구조 상한을 넘으면 `content_status=extract_error`와 고정
  `error_code=resource_limit`을 기록하고, 초과 text는 앞부분만 저장해
  `truncated=true`, `error_code=text_limit`을 기록한다. oversized/read/race/timeout/sensitive
  파일도 filename index는 유지하면서 고정 `content_status`와 `error_code`만 기록한다.
- PDF는 MIT `lopdf`로 text object만 오프라인 추출하며 page rendering, OCR, image/format
  extraction, 외부 resource 접근은 하지 않는다. image-only scanned PDF는 `no_text`,
  password/encrypted PDF는 `unsupported_encrypted`, corrupt PDF와 parser/decompression
  실패는 `extract_error`, object/page resource bound 초과는 `extract_error`와
  `resource_limit`으로 격리한다. 해당 상태에서도 filename search는 계속 제공된다.
- legacy XLS는 MIT `calamine`의 pure-Rust `Xls` reader로 worksheet 셀 값만 오프라인
  추출한다. 수식 재계산·VBA/macro·이미지·서식·외부 resource는 사용하지 않으며, XLSX와
  ODS는 확장자 dispatch에서 이 경로로 들어오지 않는다. password/encrypted workbook은
  `unsupported_encrypted`, 손상 workbook은 `extract_error`, resource-limit workbook은
  `extract_error`와 `resource_limit`으로
  격리하고 filename search는 계속 제공한다. workbook은 최대 256개 sheet와 4,000,000개
  logical cell을 허용한다. parser 진입 전 fail-closed BIFF preflight가 record 1,000,000개,
  formula 100,000개, shared string 200,000개·8,000,000자, 반복 참조로 확장되는 string
  16,000,000자와 추정 peak memory 256 MiB를 함께 제한한다.
- XLSX는 같은 MIT `calamine`의 streaming cell reader를 사용해 dense worksheet range를
  만들지 않는다. parser 진입 전에 ZIP end record의 선언 entry 수와 실제 central directory를
  각각 최대 4,096개로 확인하고, entry 32 MiB·전체 uncompressed 64 MiB, sheet 256개,
  logical/visited cell 4,000,000개, row 1,048,576개, column 16,384개를 제한한다. 표준
  `_rels/.rels`, `xl/workbook.xml`, `xl/_rels/workbook.xml.rels`만 authoritative package root로
  인정하며, 중복/unsafe ZIP path, external relationship, DTD, XML depth 128/event 1,000,000개,
  shared string 1,000,000개·8,000,000자를 calamine보다 먼저 검사한다.
- ODS도 pure-Rust `calamine::Ods`로 셀 값만 오프라인 추출한다. ZIP 선언/실제 entry 4,096개,
  entry 32 MiB·전체 uncompressed 64 MiB, sheet/row/column/cell 및 XML depth/event 상한을
  XLSX와 같은 순서로 검사한다. calamine이 dense range를 만들기 전에 row/column repeat를
  펼친 logical cell, 반복된 non-empty text/formula 16,000,000자, 기존/신규 Data·formula
  vector가 겹치는 peak memory 256 MiB를 보수적으로 계산해 repeat/clone bomb를 차단한다.
  암호화·외부 DTD·손상·ZIP/XML/resource-limit 문서는 고정 실패 metadata로 격리한다.
- 모든 spreadsheet extractor는 formula를 실행하거나 재계산하지 않는다. parser가 제공하는
  cached cell value만 일반 값처럼 취급하고 formula 원문은 FTS에 넣지 않는다. 10초 budget은
  ZIP/XML/BIFF 순회와 셀 누적에서 협력적으로 검사하며, 동기 calamine 호출 자체를 안전하게
  강제 중단하지는 않으므로 parser 진입 전 구조·메모리 상한으로 입력 작업량을 제한한다.
- `.env`, credential/netrc/npmrc/secrets 계열 파일과 private-key 확장자/이름은 읽지 않고
  `skipped_sensitive`로 기록한다. FTS snippet은 Authorization/Bearer/password/token/
  secret/API-key/private-key 형태와 독립적으로 노출된 common provider token·AWS access key·
  JWT 형태를 다시 redaction한 뒤 최대 4,096자까지만 UI에 보낸다.
  원문 내용은 app-local SQLite FTS에만 있고 network, log, telemetry, 다른 app snapshot으로
  복제하지 않는다.
- 전체/루트 재인덱스는 250개 파일 배치로 DB lock을 양보하고 UI에서 진행률·indexed/
  truncated/failed 수·마지막 시각을 확인할 수 있다. 실행 중 `Cancel`은 안전한 배치와
  파일 경계에서 협력적으로 중지하며, 이미 커밋된 부분 인덱스는 다음 `Re-index`로
  수렴한다. watcher 증분 반영도 동일 extractor와 bounds를 사용하고 읽기 전후의 크기와
  수정 시각을 열린 파일 및 경로 양쪽에서 다시 확인해 변경된 읽기는
  `changed_during_read`로 폐기한다.
- schema v2 migration은 사용자가 등록한 roots는 보존하고 파생 files/content FTS와
  metadata를 재생성한다. `content_status`, `extractor_version`, `truncated`,
  `indexed_at`, `error_code`, `encoding`, `text_chars`를 함께 저장하므로 검색 결과와
  상태 화면이 단순히 "없음"과 실패를 혼동하지 않는다.
- text/PDF/XLS/XLSX/ODS extractor 버전은 서로 독립적으로 기록된다. parser 또는 cell
  normalization 규칙이 바뀌면 stale format row만 지우고 해당 확장자 후보만 다시 읽어 다른
  형식 인덱스를 보존한다. 각 marker가 없거나 현재 버전과 달라도 해당 형식 scan을 수행하며,
  성공한 full/format-only scan만 marker를 갱신한다. partial/cancelled scan은 marker를 남기지
  않고, format-only worker 중 새 root/index 요청이 들어오면 다음 실행을 `All`로 승격한다.

## 기술

- 공용 크레이트 `crates/filesystem`(제한 순회)·`crates/search`(FTS5) 사용
- legacy XLS reader는 `calamine = 0.36.1`(MIT, pure Rust, `Cargo.lock` 고정)을 사용하고,
  `cfb = 0.7.3`(MIT, pure Rust) preflight로 calamine의 eager range allocation 전에 CFB
  Workbook stream과 BIFF record/dimension/SST 참조를 fail-closed로 확인한다. 설치물 고지에는
  lockfile 기반 transitive dependency inventory가 포함된다.
- XLSX/ODS 컨테이너 preflight는 exact `zip = 8.6.0`(MIT)과
  `quick-xml = 0.41.0`(MIT)을 default feature 없이 사용한다. 모두 앱에 정적으로 포함되어
  별도 다운로드·Office/LibreOffice 설치·network 연결이 필요하지 않다.
- 백그라운드 watcher + 주기 재스캔
- inbound Query와 search input은 UTF-8 4 KiB 및 control-character 경계를 적용하며 원문을
  로그·오류·지속 저장소에 남기지 않는다
- 파일명 검색은 기본 200개·정규식 prefilter 최대 2,000개, 내용 검색은 최대 200개로
  서로 다른 bounded result 계약을 유지한다
- 다른 앱으로 열기는 catalog capability와 설치 manifest를 모두 통과한 대상만 표시하고, 실행 직전 기존 절대 파일 경로를 재검증한다

## 데이터

- 인덱스: `%LOCALAPPDATA%\com.devbox.everythingplus\data.db`

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`
