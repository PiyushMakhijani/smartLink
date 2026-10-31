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
        ("convert", "x"),
        ("fix", "."),
        ("sync", "--manifest=.smartlink.json"),
    ] {
        let out = run(&[cmd, arg], dir.path());
        assert!(!out.status.success(), "{cmd} should not be implemented yet");
        assert!(String::from_utf8_lossy(&out.stderr).contains("plan.md"));
    }
}
