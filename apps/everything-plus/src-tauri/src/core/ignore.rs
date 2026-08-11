/// 기본 제외 규칙. 디렉터리 이름 기준 (gitignore 스타일 부분 일치).
pub fn is_ignored_dir(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | "node_modules"
            | "target"
            | "dist"
            | "dist-ssr"
            | "build"
            | ".next"
            | ".cache"
            | "__pycache__"
            | ".venv"
            | "venv"
            | ".idea"
            | ".vscode"
            | ".cargo"
            | ".npm"
            | ".local"
            | "AppData"
            | "$RECYCLE.BIN"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_common_dirs() {
        assert!(is_ignored_dir(".git"));
        assert!(is_ignored_dir("node_modules"));
        assert!(is_ignored_dir("target"));
        assert!(is_ignored_dir("dist"));
    }

    #[test]
    fn keeps_normal_dirs() {
        assert!(!is_ignored_dir("src"));
        assert!(!is_ignored_dir("projects"));
        assert!(!is_ignored_dir("Documents"));
    }
}
