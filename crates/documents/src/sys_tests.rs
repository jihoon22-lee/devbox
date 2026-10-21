use crate::fixture::{T, files};
use crate::ledger::TmpLedger;
use crate::model::DiskRev;
use crate::model::Eol;
use crate::sys::*;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

#[test]
fn private_modes_exchange_and_no_clobber_preserve_both_writers() -> T {
    // v0.9.0 wsl-helper document.rs, adapted: staging is a same-folder `.devbox-tmp-*`
    // file recorded in the data-folder ledger and removed after the exchange.
    for mode in [0o600, 0o640, 0o644] {
        let root = tempfile::tempdir()?;
        let ledger = TmpLedger::new(root.path().join("ledger"));
        let notes = root.path().join("notes");
        fs::create_dir(&notes)?;
        let target = notes.join("Case.md");
        fs::write(&target, "external")?;
        fs::set_permissions(&target, fs::Permissions::from_mode(mode))?;
        let dir = open_dir(&notes)?;
        let before = read_at(&dir, "Case.md", 1024)?.expect("present");
        assert!(matches!(
            atomic_replace(
                &dir,
                &notes,
                "Case.md",
                b"ours",
                None,
                &ledger,
                &Probe::default()
            ),
            Err(SysError::Exists)
        ));
        assert_eq!(
            fs::read_to_string(&target)?,
            "external",
            "no-replace kept the writer"
        );
        let rev = atomic_replace(
            &dir,
            &notes,
            "Case.md",
            b"ours",
            Some(&before.rev),
            &ledger,
            &Probe::default(),
        )?;
        assert_eq!(fs::read_to_string(&target)?, "ours");
        assert_eq!(fs::metadata(&target)?.permissions().mode() & 0o777, mode);
        assert_eq!(rev.size, 4);
        assert_eq!(
            files(&notes),
            std::slice::from_ref(&target),
            "{mode:o}: no temp file remains"
        );
        assert!(files(&root.path().join("ledger")).is_empty());
    }
    Ok(())
}

#[test]
fn exchange_restores_an_external_write_that_raced_the_check() -> T {
    let root = tempfile::tempdir()?;
    let ledger = TmpLedger::new(root.path().join("ledger"));
    let target = root.path().join("a.md");
    fs::write(&target, "base")?;
    let dir = open_dir(root.path())?;
    let base = read_at(&dir, "a.md", 1024)?.expect("present").rev;
    fs::write(&target, "raced external edit")?;
    match atomic_replace(
        &dir,
        root.path(),
        "a.md",
        b"ours",
        Some(&base),
        &ledger,
        &Probe::default(),
    ) {
        Err(SysError::Conflict(Some(rev))) => assert_eq!(rev.size, 19),
        other => panic!("expected a conflict, got {other:?}"),
    }
    assert_eq!(fs::read_to_string(&target)?, "raced external edit");
    assert_eq!(
        files(root.path()).len(),
        2,
        "only the file and the ledger folder"
    );
    Ok(())
}

fn names(dir: &Path, marker: &str) -> Vec<String> {
    files(dir)
        .iter()
        .filter_map(|p| p.file_name()?.to_str().map(str::to_owned))
        .filter(|n| n.contains(marker))
        .collect()
}
/// `a.md` holds "ext1" while the caller still believes it holds "base".
fn raced()
-> Result<(tempfile::TempDir, TmpLedger, std::os::fd::OwnedFd, DiskRev), Box<dyn std::error::Error>>
{
    let root = tempfile::tempdir()?;
    let ledger = TmpLedger::new(root.path().join("ledger"));
    fs::write(root.path().join("a.md"), "base")?;
    let dir = open_dir(root.path())?;
    let base = read_at(&dir, "a.md", 1024)?.expect("present").rev;
    fs::write(root.path().join("a.md"), "ext1")?;
    Ok((root, ledger, dir, base))
}
fn assert_clean(root: &Path) {
    assert!(
        names(root, ".devbox-tmp-").is_empty(),
        "no temp file is left"
    );
    assert!(
        files(&root.join("ledger")).is_empty(),
        "no ledger entry is left"
    );
}

#[test]
fn conflict_undo_keeps_a_newer_external_save_under_a_recovery_name() -> T {
    let (root, ledger, dir, base) = raced()?;
    // The editor saves again while the displaced bytes are being hashed.
    let hook = |folder: &Path, _: &str| fs::write(folder.join("a.md"), "ext2").expect("ext2");
    let probe = Probe {
        stop_after_tmp: false,
        after_exchange: Some(&hook),
    };
    let result = atomic_replace(
        &dir,
        root.path(),
        "a.md",
        b"ours",
        Some(&base),
        &ledger,
        &probe,
    );
    assert!(
        matches!(result, Err(SysError::Conflict(Some(_)))),
        "{result:?}"
    );
    assert_eq!(fs::read_to_string(root.path().join("a.md"))?, "ext1");
    let kept = names(root.path(), ".devbox-conflict-");
    assert_eq!(kept.len(), 1, "the newest external save is kept");
    assert!(kept[0].starts_with("a.md.devbox-conflict-"));
    assert_eq!(fs::read_to_string(root.path().join(&kept[0]))?, "ext2");
    assert_clean(root.path());
    Ok(())
}

#[test]
fn failed_undo_moves_the_displaced_bytes_to_a_recovery_name() -> T {
    let (root, ledger, dir, base) = raced()?;
    // The target disappears, so the undo exchange cannot run.
    let hook = |folder: &Path, _: &str| fs::remove_file(folder.join("a.md")).expect("remove");
    let probe = Probe {
        stop_after_tmp: false,
        after_exchange: Some(&hook),
    };
    let result = atomic_replace(
        &dir,
        root.path(),
        "a.md",
        b"ours",
        Some(&base),
        &ledger,
        &probe,
    );
    assert!(matches!(result, Err(SysError::Conflict(_))), "{result:?}");
    let kept = names(root.path(), ".devbox-conflict-");
    assert_eq!(kept.len(), 1);
    assert_eq!(fs::read_to_string(root.path().join(&kept[0]))?, "ext1");
    assert_clean(root.path());
    Ok(())
}

#[test]
fn unverifiable_displaced_bytes_are_restored_not_deleted() -> T {
    let (root, ledger, dir, _) = raced()?;
    if fs::metadata(root.path())?.uid() == 0 {
        return Ok(()); // root reads mode-000 files; the failure cannot be produced
    }
    let base = read_at(&dir, "a.md", 1024)?.expect("present").rev;
    let hook = |folder: &Path, tmp: &str| {
        fs::set_permissions(folder.join(tmp), fs::Permissions::from_mode(0o000)).expect("chmod")
    };
    let probe = Probe {
        stop_after_tmp: false,
        after_exchange: Some(&hook),
    };
    let result = atomic_replace(
        &dir,
        root.path(),
        "a.md",
        b"ours",
        Some(&base),
        &ledger,
        &probe,
    );
    assert!(matches!(result, Err(SysError::Conflict(_))), "{result:?}");
    let target = root.path().join("a.md");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600))?;
    assert_eq!(
        fs::read_to_string(&target)?,
        "ext1",
        "unverified bytes go back in place"
    );
    assert!(
        names(root.path(), ".devbox-conflict-").is_empty(),
        "our own bytes were verified"
    );
    assert_clean(root.path());
    Ok(())
}

#[test]
fn disk_text_round_trips_eol_and_bom() {
    let (text, eol, bom) = decode_text("\u{feff}a\r\nb\r\n").expect("utf-8");
    assert_eq!((text.as_str(), eol, bom), ("a\nb\n", Eol::Crlf, true));
    assert_eq!(
        encode_text("x\ny\n", Eol::Crlf, true),
        "\u{feff}x\r\ny\r\n".as_bytes()
    );
    assert_eq!(encode_text("x\ny", Eol::Lf, false), b"x\ny");
    let (text, eol, bom) = decode_text("lone\rcr\n").expect("utf-8");
    assert_eq!((text.as_str(), eol, bom), ("lone\rcr\n", Eol::Lf, false));
}

#[test]
fn mixed_line_endings_follow_the_majority_and_ties_choose_lf() {
    for (disk, want) in [
        ("a\r\nb\nc\n", Eol::Lf),
        ("a\r\nb\r\nc\n", Eol::Crlf),
        ("a\r\nb\n", Eol::Lf),
        ("a\r\n", Eol::Crlf),
    ] {
        let (text, eol, _) = decode_text(disk).expect("utf-8");
        assert_eq!(eol, want, "{disk:?}");
        assert!(!text.contains('\r'), "editor text is LF only: {disk:?}");
    }
    assert_eq!(encode_text("a\nb\nc\n", Eol::Lf, false), b"a\nb\nc\n");
}

#[test]
fn escaped_length_matches_serde_json() {
    for text in [
        "",
        "plain",
        "\"\\",
        "tab\tnew\nline\r",
        "\u{1}\u{1f}",
        "한글 노트",
    ] {
        let json = serde_json::to_string(text).expect("encode");
        assert_eq!(escaped_len(text), json.len() - 2, "{text:?}");
    }
}
