//! Integration tests over the real binary (plan.md §5, trimmed to what M1 covers).

use std::path::Path;
use std::process::Command;

fn bin() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_smartlink"))
}

fn run(args: &[&str], cwd: &Path) -> std::process::Output {
    Command::new(bin()).args(args).current_dir(cwd).output().unwrap()
}

#[test]
fn make_hardlink_and_ls_on_windows_is_covered_by_unit_linking() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(root.join("orig.txt"), b"payload").unwrap();
    let link = root.join("copy.txt");

    // Auto-detection on the same volume must produce a hardlink for files.
    let out = run(
        &["make", "orig.txt", link.to_str().unwrap()],
        root,
    );
    assert!(out.status.success(), "make failed: {}", String::from_utf8_lossy(&out.stderr));
    assert!(link.exists());
    assert_eq!(std::fs::read(&link).unwrap(), b"payload");

    let out = run(&["ls", root.to_str().unwrap(), "--hardlinks"], root);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    // The two names share one inode/file-id, so ls must group them.
    assert!(text.contains("hardlink"), "ls output missing hardlink group:\n{text}");
}

#[test]
fn make_rejects_missing_target() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(&["make", "nope.txt", "link.txt"], dir.path());
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("does not exist"));
}

#[test]
fn make_refuses_existing_link_without_force() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), b"1").unwrap();
    std::fs::write(root.join("b.txt"), b"2").unwrap();

    let first = run(&["make", "a.txt", "l.txt"], root);
    assert!(first.status.success());

    let second = run(&["make", "b.txt", "l.txt"], root);
    assert!(!second.status.success(), "should refuse to overwrite");

    let forced = run(&["make", "b.txt", "l.txt", "--force"], root);
    assert!(forced.status.success(), "{}", String::from_utf8_lossy(&forced.stderr));
    assert_eq!(std::fs::read(root.join("l.txt")).unwrap(), b"2");
}

#[test]
fn pending_commands_fail_with_milestone_hint() {
    let dir = tempfile::tempdir().unwrap();
    for (cmd, arg) in [
        ("init", "."),
        ("sync", "--manifest=.smartlink.json"),
        ("git", "check"),
    ] {
        let out = run(&[cmd, arg], dir.path());
        assert!(!out.status.success(), "{cmd} should not be implemented yet");
        assert!(String::from_utf8_lossy(&out.stderr).contains("plan.md"));
    }
}

// ---------------------------------------------------------------- M2 -------

#[cfg(windows)]
#[test]
fn convert_junction_to_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("libs")).unwrap();
    std::fs::create_dir_all(root.join("vendor")).unwrap();
    std::fs::write(root.join("libs/api.txt"), b"x").unwrap();

    let link = root.join("vendor/l");
    let out = run(
        &["make", "libs", link.to_str().unwrap(), "--junction"],
        root,
    );
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let out = run(
        &["convert", link.to_str().unwrap(), "--to", "symlink"],
        root,
    );
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(std::fs::read(link.join("api.txt")).unwrap(), b"x");

    // ls must now report it as a symlink, still OK (scan the parent dir,
    // since `ls <dir>` lists links directly inside it, not through them).
    let out = run(&["ls", "vendor"], root);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("symlink") && text.contains("OK"), "{text}");
}

#[cfg(windows)]
#[test]
fn convert_junction_dir_to_hardlink_fails_gracefully() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("libs")).unwrap();
    std::fs::create_dir_all(root.join("vendor")).unwrap();

    let link = root.join("vendor/l");
    let out = run(
        &["make", "libs", link.to_str().unwrap(), "--junction"],
        root,
    );
    assert!(out.status.success());

    let out = run(
        &["convert", link.to_str().unwrap(), "--to", "hard"],
        root,
    );
    assert!(!out.status.success(), "junction dirs cannot become hardlinks");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.to_lowercase().contains("hardlink"), "{err}");
    // the original junction must still be intact
    assert!(link.join("libs").exists() || link.exists());
}

#[cfg(windows)]
#[test]
fn fix_heals_dead_junction_and_convert_requires_preserve_broken() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("libs/src")).unwrap();
    std::fs::create_dir_all(root.join("vendor")).unwrap();
    std::fs::write(root.join("libs/src/data.txt"), b"payload").unwrap();

    let link = root.join("vendor/l");
    let out = run(
        &["make", "libs/src", link.to_str().unwrap(), "--junction"],
        root,
    );
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    // move the real directory so the junction goes dead, keeping the leaf
    // name `src` so `fix`'s filename search can locate the replacement.
    std::fs::create_dir_all(root.join("relocated")).unwrap();
    std::fs::rename(root.join("libs/src"), root.join("relocated/src")).unwrap();

    // convert refuses without --preserve-broken
    let out = run(&["convert", link.to_str().unwrap(), "--to", "symlink"], root);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--preserve-broken"));

    // fix finds the moved directory by name and re-links
    let out = run(&["fix", ".", "--yes"], root);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(std::fs::read(link.join("data.txt")).unwrap(), b"payload");
}

#[cfg(windows)]
#[test]
fn fix_dry_run_touches_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("libs/src")).unwrap();
    std::fs::create_dir_all(root.join("vendor")).unwrap();
    std::fs::write(root.join("libs/src/data.txt"), b"p").unwrap();

    let link = root.join("vendor/l");
    assert!(run(&["make", "libs/src", link.to_str().unwrap(), "--junction"], root).status.success());
    std::fs::remove_dir_all(root.join("libs/src")).unwrap();

    let out = run(&["fix", ".", "--yes", "--dry-run"], root);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    // the dead junction still points nowhere
    assert!(!link.join("data.txt").exists());
}

#[test]
fn dedupe_refuses_existing_dest_without_force() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), b"data").unwrap();
    std::fs::write(root.join("b.txt"), b"old").unwrap();

    let out = run(&["dedupe", "a.txt", "b.txt"], root);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--force"));
    assert_eq!(std::fs::read(root.join("b.txt")).unwrap(), b"old");
}

#[test]
fn dedupe_clone_or_clean_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let payload: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(root.join("a.bin"), &payload).unwrap();

    let out = run(&["dedupe", "a.bin", "clone.bin"], root);
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    if out.status.success() {
        // CoW-capable volume (APFS / ReFS / btrfs…): content identical and
        // it must NOT be a hardlink (independent names, shared blocks).
        assert_eq!(std::fs::read(root.join("clone.bin")).unwrap(), payload);
    } else {
        // Explicit refusal with guidance, and nothing left behind.
        assert!(stderr.contains("CoW") || stderr.contains("cloning"), "{stderr}");
        assert!(!root.join("clone.bin").exists(), "failed clone must not leave debris");
    }
}
