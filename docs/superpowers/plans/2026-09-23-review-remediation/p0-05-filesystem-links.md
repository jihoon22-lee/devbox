# P0-05 OneDrive·Dev Drive 파일 처리 — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4를 읽는다.

**Goal:** 사용자가 OneDrive 동기화 폴더·WOF 압축 폴더·Dev Drive(ReFS)의 프로젝트나 노트 폴더를 쓸 수 있게 한다. 이름을 다른 위치로 돌리는 링크(심볼릭 링크·junction)는 지금처럼 거부한다(D11).

**Architecture:** 규칙을 "모든 reparse point 거부"에서 "이름 대리(name-surrogate) reparse point만 거부"로 바꾼다. Rust 표준 라이브러리의 Windows `FileType::is_symlink()`가 바로 이 기준(태그의 0x20000000 비트)이므로 경로 검사는 그것을 쓰고, 핸들 검사는 `FILE_ATTRIBUTE_TAG_INFO`로 같은 비트를 본다. 파일 비교는 `FILE_ID_INFO`(64비트 볼륨 + 128비트 ID)를 함께 사용한다. 설치 기록에 사용되는 `components()`는 v0.8.1의 볼륨32·파일64 값을 유지하고, 콘텐츠용 `content_components()`는 확장 ID를 반영한다. OneDrive placeholder를 읽을 때는 reparse 핸들을 버리고 일반 핸들로 다시 열어 동기화 드라이버가 내용을 내려받게 한다.

**Tech Stack:** Rust, `windows` 0.61 (`Win32_Storage_FileSystem`)

**Spec:** `review.md` §3 B7 · `00-roadmap.md` D11

> **리뷰 정정:** 제품(v0.8) 모드의 기본 노트 저장소는 제품 데이터 폴더 아래 `vault`다(`crates/knowledge-vault-engine/src/component.rs:103-107`). 리뷰에 적은 `Documents\Knowledge`는 standalone 엔진의 기본값이다. 따라서 OneDrive 영향은 사용자가 OneDrive 안의 폴더를 vault나 프로젝트로 연결할 때 생긴다.

## Global Constraints

- `00-roadmap.md` §3 전부 적용.
- 이 PR은 사용자 콘텐츠 경로만 바꾼다: `crates/filesystem`, `crates/knowledge-vault-engine/src/core/vault.rs`, `crates/repositories-engine/src/core/dependency_lens.rs`. 제품 데이터 폴더(LOCALAPPDATA) 전용 검사(`webhook-core` fixtures, `integration`, `editor-engine` LSP 설치 경로, Workspace `wsl_helper`, API Studio `owned_copy`, Knowledge `document_wsl`)는 바꾸지 않는다.
- `FilesystemIdentity` 자체는 직렬화하지 않지만 `components()`는 설치 namespace·suite-owner·복구·제거 기록에 저장된다. 그 값과 시그니처는 바꾸지 않는다. Windows에서 같은 핸들의 확장 ID를 별도로 보관해 객체 비교에 사용하고, 콘텐츠 proof·revision은 `content_components()`를 쓴다. 기존 데이터 이동·변환은 하지 않는다.
- 2026-09-25 재개 지시에 따른 호환 보완: Task 2에 기존 설치 키 보존과 128비트 상위 ID 구분 회귀를 추가한다. Task 4는 콘텐츠 proof 생산자·소비자 및 revision을 함께 맞춘다. 설치 namespace 소비자는 기존 API를 유지한다. 아래 예시의 전역 ID 교체 대신 이 호환 계약을 우선 적용한다.

## Review Focus

1. junction(`mklink /J`)으로 만든 폴더를 프로젝트·vault로 지정 → 지금처럼 거부된다. (Task 3 Windows 테스트)
2. WOF 압축 파일(`compact /c /exe:xpress4k`) → 열고 읽을 수 있다. (Task 3 Windows 테스트)
3. 같은 파일을 두 번 식별 → 같은 identity, 다른 파일 → 다른 identity (NTFS 기존 테스트 유지). (Task 1, 기존 테스트)
4. FileIdInfo를 지원하지 않는 파일시스템(FAT/일부 네트워크) → 이전 방식으로 대체되어 동작한다. (Task 2 구현)
5. OneDrive "온라인 전용" 파일을 연 뒤 읽기 → 내용이 내려받아져 읽힌다. (Windows 실기)

## Branch · PR

- 묶음: **B2** — 브랜치 `fix/suite/files-webhooks-logs-describe`, PR 제목 `fix(suite): cloud files, webhook bodies, operation logs and one describe per session`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `fix(crates): allow OneDrive and Dev Drive files while still rejecting links`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

## File Structure

| 파일 | 변경 | 책임 |
|---|---|---|
| `crates/filesystem/src/links.rs` | 생성 | 이식 가능한 링크·ID 판단 함수 |
| `crates/filesystem/src/lib.rs` | 수정 | `mod links`, `ensure_no_links`, Windows identity·open 경로 |
| `crates/knowledge-vault-engine/src/core/vault.rs` | 수정 | 링크 판단·Windows identity를 공용 함수로 |
| `crates/repositories-engine/src/core/dependency_lens.rs:928-940` | 수정 | 공용 링크 판단 사용 |

---

### Task 1: 이식 가능한 판단 함수

**Interfaces (Produces, `devbox_filesystem`에서 재수출):**
- `pub fn is_link_metadata(metadata: &std::fs::Metadata) -> bool`
- `pub const fn is_name_surrogate_tag(tag: u32) -> bool`
- `pub fn object_from_file_id(id: [u8; 16]) -> u64`

- [ ] **Step 1: 실패하는 테스트** — `crates/filesystem/src/links.rs`

```rust
//! Link rules shared by every user-content path check.
//!
//! A *link* names another location: a Unix symlink, or a Windows reparse
//! point whose tag has the name-surrogate bit (symbolic link, junction, mount
//! point). Storage-layer reparse points — OneDrive/Cloud Files placeholders,
//! WOF compression, data deduplication — are ordinary files and directories.

/// `metadata` must come from `symlink_metadata`. On Windows the standard
/// library reports `is_symlink()` exactly for name-surrogate reparse points.
pub fn is_link_metadata(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

/// `IsReparseTagNameSurrogate` from the Windows SDK.
pub const fn is_name_surrogate_tag(tag: u32) -> bool {
    tag & 0x2000_0000 != 0
}

/// Fold a 128-bit `FILE_ID_INFO` identifier into the 64-bit identity
/// component. NTFS identifiers fit in the low half and keep their exact
/// value; ReFS (Dev Drive) identifiers use all 128 bits.
pub fn object_from_file_id(id: [u8; 16]) -> u64 {
    let low = u64::from_le_bytes(id[..8].try_into().expect("8 bytes"));
    let high = u64::from_le_bytes(id[8..].try_into().expect("8 bytes"));
    if high == 0 {
        return low;
    }
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_name_surrogate_tags_are_links() {
        assert!(is_name_surrogate_tag(0xA000_000C)); // IO_REPARSE_TAG_SYMLINK
        assert!(is_name_surrogate_tag(0xA000_0003)); // IO_REPARSE_TAG_MOUNT_POINT (junction)
        assert!(!is_name_surrogate_tag(0x9000_001A)); // IO_REPARSE_TAG_CLOUD
        assert!(!is_name_surrogate_tag(0x9000_601A)); // IO_REPARSE_TAG_CLOUD_6 (OneDrive)
        assert!(!is_name_surrogate_tag(0x8000_0017)); // IO_REPARSE_TAG_WOF
        assert!(!is_name_surrogate_tag(0x8000_0013)); // IO_REPARSE_TAG_DEDUP
    }

    #[test]
    fn ntfs_ids_keep_their_value_and_refs_ids_use_all_bits() {
        let mut ntfs = [0u8; 16];
        ntfs[..8].copy_from_slice(&42u64.to_le_bytes());
        assert_eq!(object_from_file_id(ntfs), 42);

        let mut first = [0u8; 16];
        let mut second = [0u8; 16];
        first[..8].copy_from_slice(&7u64.to_le_bytes());
        second[..8].copy_from_slice(&7u64.to_le_bytes());
        first[8..].copy_from_slice(&1u64.to_le_bytes());
        second[8..].copy_from_slice(&2u64.to_le_bytes());
        assert_ne!(object_from_file_id(first), object_from_file_id(second));
        assert_eq!(object_from_file_id(first), object_from_file_id(first));
    }

    #[cfg(unix)]
    #[test]
    fn unix_symlinks_are_links_and_files_are_not() {
        let dir = std::env::temp_dir().join(format!("links-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("file"), b"x").unwrap();
        std::os::unix::fs::symlink(dir.join("file"), dir.join("link")).unwrap();
        assert!(!is_link_metadata(&std::fs::symlink_metadata(dir.join("file")).unwrap()));
        assert!(is_link_metadata(&std::fs::symlink_metadata(dir.join("link")).unwrap()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
```

- [ ] **Step 2: 등록** — `crates/filesystem/src/lib.rs`의 `pub mod walk;` 아래에 `pub mod links;`, 재수출 목록에 `pub use links::{is_link_metadata, is_name_surrogate_tag, object_from_file_id};`를 추가한다.

- [ ] **Step 3: 통과 확인** — Run: `source ~/.cargo/env && cargo test -p filesystem --lib links` → PASS.

- [ ] **Step 4: 커밋**

```bash
git add crates/filesystem/src/links.rs crates/filesystem/src/lib.rs
git commit -m "feat(crates): define link rules that exclude storage reparse points"
```

---

### Task 2: filesystem 크레이트 경로·핸들 검사 교체

**Interfaces (Produces):** `#[cfg(windows)] pub fn windows_file_id(handle: std::os::windows::io::RawHandle) -> std::io::Result<(u64, u64)>` — (볼륨, 객체) 쌍. 다른 크레이트가 같은 식별 규칙을 쓰기 위해 재사용한다.

- [ ] **Step 1: `ensure_no_links` 수정** (`lib.rs:231-269`) — 조상 순회 루프의 판단을 아래로 바꾸고, `FILE_ATTRIBUTE_REPARSE_POINT` 상수·`MetadataExt` import를 삭제한다. 오류 메시지는 그대로 둔다.

```rust
    for component in ancestors {
        let metadata = fs::symlink_metadata(component)?;
        if is_link_metadata(&metadata) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path contains a symbolic link or reparse point",
            ));
        }
    }
```

- [ ] **Step 2: Windows 식별 함수 추가** — `lib.rs`의 `opened_filesystem_identity` 위에 추가한다.

```rust
#[cfg(windows)]
struct WindowsObject {
    scope: u64,
    object: u64,
    attributes: u32,
}

#[cfg(windows)]
fn windows_object(raw: std::os::windows::io::RawHandle) -> io::Result<WindowsObject> {
    use windows::Win32::{
        Foundation::{HANDLE, WIN32_ERROR},
        Storage::FileSystem::{
            FileIdInfo, GetFileInformationByHandle, GetFileInformationByHandleEx,
            BY_HANDLE_FILE_INFORMATION, FILE_ID_INFO,
        },
    };
    let handle = HANDLE(raw);
    let to_io = |error: windows::core::Error| {
        WIN32_ERROR::from_error(&error)
            .map(|code| io::Error::from_raw_os_error(code.0 as i32))
            .unwrap_or_else(|| io::Error::other(error))
    };
    let mut basic = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(handle, &mut basic) }.map_err(to_io)?;
    let mut full = FILE_ID_INFO::default();
    let extended = unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileIdInfo,
            (&mut full as *mut FILE_ID_INFO).cast(),
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
    };
    let (scope, object) = match extended {
        Ok(()) => (full.VolumeSerialNumber, object_from_file_id(full.FileId.Identifier)),
        // FAT and some redirectors do not implement FileIdInfo.
        Err(_) => (
            u64::from(basic.dwVolumeSerialNumber),
            (u64::from(basic.nFileIndexHigh) << 32) | u64::from(basic.nFileIndexLow),
        ),
    };
    Ok(WindowsObject { scope, object, attributes: basic.dwFileAttributes })
}

/// Name-surrogate check for an already-open handle.
#[cfg(windows)]
fn windows_handle_is_link(raw: std::os::windows::io::RawHandle, attributes: u32) -> io::Result<bool> {
    use windows::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{
            FileAttributeTagInfo, GetFileInformationByHandleEx, FILE_ATTRIBUTE_REPARSE_POINT,
            FILE_ATTRIBUTE_TAG_INFO,
        },
    };
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 == 0 {
        return Ok(false);
    }
    let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
    unsafe {
        GetFileInformationByHandleEx(
            HANDLE(raw),
            FileAttributeTagInfo,
            (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    }
    .map_err(|error| io::Error::other(error))?;
    Ok(is_name_surrogate_tag(tag.ReparseTag))
}

/// Volume and object identity for a caller-owned Windows handle, using the
/// same rules as [`filesystem_identity`].
#[cfg(windows)]
pub fn windows_file_id(handle: std::os::windows::io::RawHandle) -> io::Result<(u64, u64)> {
    windows_object(handle).map(|object| (object.scope, object.object))
}
```

(`windows` 0.61의 `HANDLE`은 `HANDLE(*mut c_void)`이다. `RawHandle`도 `*mut c_void`라 그대로 감싼다. 시그니처가 다르면 `HANDLE(raw as _)`로 맞춘다.)

- [ ] **Step 3: `opened_filesystem_identity`의 Windows 분기 교체** (`lib.rs:124-153`)

```rust
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY;
        let raw = handle.as_raw_handle();
        let object = windows_object(raw)?;
        if windows_handle_is_link(raw, object.attributes)?
            || (object.attributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0) != directory
        {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "unexpected file type"));
        }
        Ok(FilesystemIdentity { scope: object.scope, object: object.object })
    }
```

- [ ] **Step 4: 내용 읽기용 재열기** — `open_object`의 Windows 분기 끝(현재 `let identity = opened_filesystem_identity(&handle, directory)?; Ok((handle, identity))`)을 아래로 바꾼다. 비-링크 reparse 파일(OneDrive placeholder 등)을 내용 읽기용으로 열었으면, reparse 핸들 대신 일반 핸들로 다시 열어 동기화 드라이버가 내용을 준비하게 하고 같은 객체인지 확인한다.

```rust
    let identity = opened_filesystem_identity(&handle, directory)?;
    #[cfg(windows)]
    if _read_contents && !directory {
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        use windows::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        if handle.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
            let reopened = OpenOptions::new()
                .read(true)
                .share_mode((FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE).0)
                .open(path)?;
            if opened_filesystem_identity(&reopened, false)? != identity {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "object changed while opening"));
            }
            return Ok((reopened, identity));
        }
    }
    Ok((handle, identity))
```

(`handle.metadata()`는 핸들 기준이라 경로를 다시 따라가지 않는다. 사용하지 않는 import가 생기면 지운다.)

- [ ] **Step 5: 기존 테스트 확인** — Run: `cargo test -p filesystem` → PASS (Linux). Windows 분기는 CI `Rust (Windows)`에서 기존 `identity_tests`(긴 경로·별칭·교체 감지)가 검증한다.

- [ ] **Step 6: 커밋**

```bash
git add crates/filesystem/src/lib.rs
git commit -m "fix(crates): accept storage reparse points and use 128-bit Windows file IDs"
```

---

### Task 3: Windows 회귀 테스트 (CI Windows 잡에서 실행)

- [ ] **Step 1: 테스트 추가** — `crates/filesystem/src/lib.rs`의 `mod identity_tests` 끝에 추가한다.

```rust
    #[cfg(windows)]
    #[test]
    fn windows_junctions_are_still_rejected() {
        let root = fixture_root();
        let target = root.join("target");
        let junction = root.join("junction");
        fs::create_dir(&target).unwrap();
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&target)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(super::ensure_no_links(&junction).is_err());
        assert!(super::ensure_no_links(junction.join("child")).is_err());
        assert!(filesystem_identity(&junction, true).is_err());
        let _ = fs::remove_dir(&junction);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn windows_wof_compressed_files_are_ordinary_files() {
        use std::io::Read;
        use std::os::windows::fs::MetadataExt;
        let root = fixture_root();
        let file = root.join("compressed.bin");
        fs::write(&file, vec![b'a'; 256 * 1024]).unwrap();
        let _ = std::process::Command::new("compact.exe")
            .args(["/C", "/EXE:XPRESS4K"])
            .arg(&file)
            .status();
        let compressed = fs::symlink_metadata(&file).unwrap().file_attributes() & 0x400 != 0;
        if !compressed {
            eprintln!("skipping: WOF compression is unavailable on this volume");
            let _ = fs::remove_dir_all(root);
            return;
        }
        super::ensure_no_links(&file).unwrap();
        let (mut handle, identity) = open_filesystem_object(&file, false).unwrap();
        assert_eq!(identity, filesystem_identity(&file, false).unwrap());
        let mut bytes = Vec::new();
        handle.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes.len(), 256 * 1024);
        drop(handle);
        let _ = fs::remove_dir_all(root);
    }
```

- [ ] **Step 2: 커밋** — (Windows에서만 실행되므로 로컬 실행은 없다. `cargo check -p filesystem --tests`로 Linux 컴파일만 확인.)

```bash
git add crates/filesystem/src/lib.rs
git commit -m "test(crates): pin junction rejection and WOF acceptance on Windows"
```

---

### Task 4: Knowledge vault와 의존성 분석이 같은 규칙을 쓰도록

- [ ] **Step 1: Knowledge vault** — `crates/knowledge-vault-engine/src/core/vault.rs`
  - `fn is_link_or_reparse(metadata)`의 본문 전체를 `devbox_filesystem::is_link_metadata(metadata)`로 바꾼다(함수 이름은 호출부가 많으므로 유지).
  - `enum FileIdentity`의 `Windows { volume: Option<u32>, file_index: Option<u64> }`를 `Windows { volume: Option<u64>, file_index: Option<u64> }`로 바꾼다.
  - `fn windows_handle_identity(handle)`의 본문을 아래로 바꾼다.

```rust
#[cfg(windows)]
fn windows_handle_identity(handle: ::windows::Win32::Foundation::HANDLE) -> FileIdentity {
    match devbox_filesystem::windows_file_id(handle.0 as std::os::windows::io::RawHandle) {
        Ok((volume, file_index)) => FileIdentity::Windows { volume: Some(volume), file_index: Some(file_index) },
        Err(_) => FileIdentity::Windows { volume: None, file_index: None },
    }
}
```

  - `rg -n "0x400|FILE_ATTRIBUTE_REPARSE_POINT" crates/knowledge-vault-engine/src/core/vault.rs` 결과가 남으면(루트 lease 쪽) 같은 규칙으로 바꾼다: 핸들 기반이면 `windows_handle_identity`와 함께 `devbox_filesystem::windows_file_id`를 쓰고, 속성 비트만 보는 곳은 삭제한다(이름 대리 여부는 경로 단계에서 이미 `is_link_or_reparse`로 걸러진다).

- [ ] **Step 2: 의존성 분석** — `crates/repositories-engine/src/core/dependency_lens.rs:928-940`의 `fn is_link_metadata` 본문을 `devbox_filesystem::is_link_metadata(metadata)`로 바꾼다.

- [ ] **Step 3: 확인** — Run: `cargo test -p devbox-knowledge-vault-engine --lib && cargo test -p devbox-repositories-engine --lib` → PASS.

- [ ] **Step 4: 커밋**

```bash
git add crates/knowledge-vault-engine crates/repositories-engine
git commit -m "fix(crates): use the shared link rule for vaults and dependency manifests"
```

---

### Task 5: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9. CI `Rust (Windows)`에서 Task 3 테스트가 실제로 실행되었는지(건너뜀 메시지 여부) 로그를 확인해 PR 본문에 적는다.
- [ ] Windows 실기 체크리스트(사용자 확인 대기로 기록, 집 PC):
  1. OneDrive 동기화 폴더 안의 폴더를 Knowledge vault로 연결하고 노트를 열고 저장한다(OneDrive가 없으면 "미실행").
  2. 같은 폴더의 노트 하나를 "온라인 전용"으로 바꾼 뒤 Knowledge에서 열어 내용이 보이는지 확인한다.
  3. `mklink /J`로 만든 폴더를 Workspace 프로젝트로 등록하면 거부되는지 확인한다.
  4. Dev Drive가 있으면 그 안의 폴더를 Workspace 프로젝트로 등록하고 파일 편집·저장이 되는지 확인한다(없으면 "미실행").
