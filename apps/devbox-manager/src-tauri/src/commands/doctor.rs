//! 환경 진단 (§15.4, Stage 5 — Devbox Manager 탭으로 먼저 검증).
//! read-only. 자동 설치·registry 수정·WSL reset을 하지 않는다.

use crate::core::catalog::CatalogApp;
use serde::Serialize;
use std::process::Command;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

const CATALOG_JSON: &str = include_str!("../../../../catalog.json");

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosisItem {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

fn version_of(cmd: &str, args: &[&str]) -> Option<String> {
    let mut c = Command::new(cmd);
    c.args(args);
    #[cfg(target_os = "windows")]
    c.creation_flags(0x0800_0000);
    let out = c.output().ok()?;
    if !out.status.success() {
        return None;
    }
    // `wsl.exe --version`/`-l -v`는 UTF-16LE로 출력된다 (공용 crates/wsl 디코더).
    let text = devbox_wsl::output::decode_output(&out.stdout);
    Some(text.lines().next().unwrap_or("").trim().to_string())
}

/// 전체 진단 실행 (read-only).
#[tauri::command]
pub fn run_diagnosis() -> Vec<DiagnosisItem> {
    let mut items = Vec::new();

    // WSL
    let wsl =
        version_of("wsl.exe", &["--version"]).or_else(|| version_of("wsl.exe", &["-l", "-v"]));
    match wsl {
        Some(v) => items.push(DiagnosisItem {
            name: "wsl".into(),
            ok: true,
            detail: v,
        }),
        None => items.push(DiagnosisItem {
            name: "wsl".into(),
            ok: false,
            detail: "wsl.exe 조회 불가 — WSL 설치 필요".into(),
        }),
    }

    // Git
    match version_of("git", &["--version"]) {
        Some(v) => items.push(DiagnosisItem {
            name: "git".into(),
            ok: true,
            detail: v,
        }),
        None => items.push(DiagnosisItem {
            name: "git".into(),
            ok: false,
            detail: "git 미설치".into(),
        }),
    }

    // Node / pnpm
    match version_of("node", &["--version"]) {
        Some(v) => items.push(DiagnosisItem {
            name: "node".into(),
            ok: true,
            detail: v,
        }),
        None => items.push(DiagnosisItem {
            name: "node".into(),
            ok: false,
            detail: "node 미설치".into(),
        }),
    }
    match version_of("pnpm", &["--version"]) {
        Some(v) => items.push(DiagnosisItem {
            name: "pnpm".into(),
            ok: true,
            detail: v,
        }),
        None => items.push(DiagnosisItem {
            name: "pnpm".into(),
            ok: false,
            detail: "pnpm 미설치".into(),
        }),
    }

    // Rust
    match version_of("rustc", &["--version"]) {
        Some(v) => items.push(DiagnosisItem {
            name: "rustc".into(),
            ok: true,
            detail: v,
        }),
        None => items.push(DiagnosisItem {
            name: "rustc".into(),
            ok: false,
            detail: "rustc 미설치".into(),
        }),
    }
    match version_of("cargo", &["--version"]) {
        Some(v) => items.push(DiagnosisItem {
            name: "cargo".into(),
            ok: true,
            detail: v,
        }),
        None => items.push(DiagnosisItem {
            name: "cargo".into(),
            ok: false,
            detail: "cargo 미설치".into(),
        }),
    }

    // Docker
    match version_of("docker", &["--version"]) {
        Some(v) => items.push(DiagnosisItem {
            name: "docker".into(),
            ok: true,
            detail: v,
        }),
        None => items.push(DiagnosisItem {
            name: "docker".into(),
            ok: false,
            detail: "docker CLI 미설치".into(),
        }),
    }

    // devbox 앱 데이터 디렉터리 + 카탈로그 정합
    let catalog = crate::core::catalog::parse_catalog(CATALOG_JSON).unwrap_or_else(|_| {
        crate::core::catalog::Catalog {
            schema_version: 0,
            apps: vec![],
        }
    });
    let mut dir_ok = 0;
    for app in &catalog.apps {
        let dir = std::env::var("LOCALAPPDATA")
            .map(|base| std::path::PathBuf::from(base).join(&app.identifier))
            .unwrap_or_default();
        if dir.is_dir() {
            dir_ok += 1;
        }
    }
    items.push(DiagnosisItem {
        name: "devbox-data".into(),
        ok: dir_ok > 0,
        detail: format!(
            "카탈로그 {}개 · 데이터 디렉터리 존재 {}개",
            catalog.apps.len(),
            dir_ok
        ),
    });

    // 카탈로그 identifier 정합 (com.devbox. 접두사)
    let bad_ids: Vec<&String> = catalog
        .apps
        .iter()
        .map(|a: &CatalogApp| &a.identifier)
        .filter(|id| !id.starts_with("com.devbox."))
        .collect();
    items.push(DiagnosisItem {
        name: "catalog-ids".into(),
        ok: bad_ids.is_empty(),
        detail: if bad_ids.is_empty() {
            "모든 identifier가 com.devbox.*".into()
        } else {
            format!("비정상: {bad_ids:?}")
        },
    });

    items
}
