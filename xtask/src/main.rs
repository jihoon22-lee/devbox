mod ts;
use anyhow::{Context, Result, bail};
use std::path::Path;

const USAGE: &str = "usage: cargo run -p xtask -- gen-ts [--check]";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let check = parse_args(&args)?;
    let mut exporter = ts::TsExporter::new();
    devbox_protocol::system::export(&mut exporter);
    devbox_projects::export(&mut exporter);
    let out = exporter.render()?;
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../app/src/rpc/gen/rpc.ts");
    update_file(&path, &out, check)?;
    println!(
        "gen-ts: {}",
        if check {
            "up to date"
        } else {
            "wrote app/src/rpc/gen/rpc.ts"
        }
    );
    Ok(())
}

fn parse_args(args: &[String]) -> Result<bool> {
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["gen-ts"] => Ok(false),
        ["gen-ts", "--check"] => Ok(true),
        _ => bail!(USAGE),
    }
}

fn update_file(path: &Path, out: &str, check: bool) -> Result<()> {
    if check {
        let current = std::fs::read_to_string(path)
            .with_context(|| format!("cannot check {}; run `pnpm gen`", path.display()))?;
        if current != out {
            bail!(
                "{} 가 선언과 다릅니다. `pnpm gen`을 실행해 커밋하세요.",
                path.display()
            );
        }
    } else {
        std::fs::create_dir_all(path.parent().context("gen dir")?)?;
        std::fs::write(path, out)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_the_complete_command_line() {
        let args = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(!parse_args(&args(&["gen-ts"])).expect("gen-ts parses"));
        assert!(parse_args(&args(&["gen-ts", "--check"])).expect("gen-ts check parses"));
        for invalid in [
            vec![],
            vec!["other"],
            vec!["gen-ts", "--unknown"],
            vec!["gen-ts", "--check", "--check"],
        ] {
            assert_eq!(
                parse_args(&args(&invalid))
                    .expect_err("invalid arguments are rejected")
                    .to_string(),
                USAGE
            );
        }
    }

    #[test]
    fn generation_creates_directories_and_check_never_writes() {
        let dir = tempfile::tempdir().expect("create fixture directory");
        let path = dir.path().join("nested/rpc.ts");
        assert!(update_file(&path, "contract\n", true).is_err());
        assert!(!path.exists());
        update_file(&path, "contract\n", false).expect("update or verify fixture content");
        update_file(&path, "contract\n", true).expect("update or verify fixture content");
        assert!(update_file(&path, "changed\n", true).is_err());
        assert_eq!(
            std::fs::read_to_string(&path).expect("read generated fixture"),
            "contract\n"
        );
        update_file(&path, "changed\n", false).expect("update or verify fixture content");
        update_file(&path, "changed\n", true).expect("update or verify fixture content");
    }
}
