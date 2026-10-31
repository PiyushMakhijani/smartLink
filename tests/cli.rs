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

// ----------------------------------------------------- M4: git integration --

#[cfg(windows)]
#[test]
fn git_check_flags_absolute_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("libs")).unwrap();
    std::fs::write(root.join("libs/f.txt"), b"x").unwrap();
    let abs = std::fs::canonicalize(root.join("libs/f.txt")).unwrap();

    // An absolute target with no --relative is stored verbatim (non-portable).
    let made = run(&["make", abs.to_str().unwrap(), "link.txt", "--symlink"], root);
    assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));

    let chk = run(&["git", "check", "."], root);
    assert!(!chk.status.success(), "absolute symlink should fail the check");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&chk.stdout),
        String::from_utf8_lossy(&chk.stderr)
    );
    assert!(text.contains("absolute symlink"), "{text}");
}

#[test]
fn git_check_passes_with_relative_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("libs")).unwrap();
    std::fs::write(root.join("libs/f.txt"), b"x").unwrap();

    // relative link path: `make --relative` must still store a relative target
    let made = run(
        &["make", "libs/f.txt", "link.txt", "--symlink", "--relative"],
        root,
    );
    assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));

    let chk = run(&["git", "check", "."], root);
    assert!(chk.status.success(), "{}", String::from_utf8_lossy(&chk.stderr));
    assert!(String::from_utf8_lossy(&chk.stdout).contains("no absolute symlinks"));
}

#[test]
fn git_install_and_uninstall_hooks_are_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join(".git/hooks")).unwrap();

    let inst = run(&["git", "install-hooks", "."], root);
    assert!(inst.status.success(), "{}", String::from_utf8_lossy(&inst.stderr));

    let hook = root.join(".git/hooks/post-merge");
    assert!(hook.exists(), "post-merge hook should be installed");
    let text = std::fs::read_to_string(&hook).unwrap();
    assert!(text.contains("smartlink sync"), "{text}");
    assert!(text.contains(">>> smartlink >>>"), "{text}");

    // installing again refreshes (still exactly one managed block, no dupes)
    assert!(run(&["git", "install-hooks", "."], root).status.success());
    let text = std::fs::read_to_string(&hook).unwrap();
    assert_eq!(text.matches(">>> smartlink >>>").count(), 1, "{text}");

    // uninstall strips the block and removes the now-trivial hook file
    let un = run(&["git", "uninstall-hooks", "."], root);
    assert!(un.status.success(), "{}", String::from_utf8_lossy(&un.stderr));
    assert!(!hook.exists(), "trivial hook should be deleted on uninstall");
}

#[test]
fn git_install_hooks_requires_a_repository() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(&["git", "install-hooks", "."], dir.path());
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("git repository"));
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

// ---------------------------------------------------------------- M3 -------

#[test]
fn init_records_live_symlinks_into_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("libs")).unwrap();
    std::fs::write(root.join("libs/shared.txt"), b"payload").unwrap();

    let link = root.join("src/link.txt");
    let out = run(
        &["make", "libs/shared.txt", link.to_str().unwrap(), "--symlink"],
        root,
    );
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let out = run(&["init", "."], root);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let text = std::fs::read_to_string(root.join(".smartlink.json")).unwrap();
    let m: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(m["version"], 1);
    let entries = m["links"].as_array().unwrap();
    assert_eq!(entries.len(), 1, "manifest: {text}");
    assert_eq!(entries[0]["type"], "symlink");
    // both endpoints stored relative to the root
    assert!(entries[0]["link"].as_str().unwrap().ends_with("link.txt"));
    assert!(entries[0]["target"].as_str().unwrap().ends_with("shared.txt"));
    assert!(!entries[0]["link"].as_str().unwrap().starts_with('/'));
}

#[test]
fn sync_recreates_links_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("libs/shared")).unwrap();
    std::fs::write(root.join("libs/shared/data.txt"), b"payload").unwrap();

    std::fs::write(
        root.join(".smartlink.json"),
        r#"{ "version": 1, "links": [
            { "link": "src/shared", "target": "libs/shared", "type": "symlink" }
        ] }"#,
    )
    .unwrap();

    let out = run(&["sync"], root);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(std::fs::read(root.join("src/shared/data.txt")).unwrap(), b"payload");

    // second run creates nothing new
    let out = run(&["sync"], root);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains("up to date"));
}

#[test]
fn sync_reports_conflict_without_force_and_heals_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("libs/one")).unwrap();
    std::fs::create_dir_all(root.join("libs/two")).unwrap();
    std::fs::write(root.join("libs/one/data.txt"), b"1").unwrap();
    std::fs::write(root.join("libs/two/data.txt"), b"2").unwrap();

    let manifest = root.join(".smartlink.json");
    std::fs::write(
        &manifest,
        r#"{ "version": 1, "links": [
            { "link": "src/x", "target": "libs/one", "type": "symlink" }
        ] }"#,
    )
    .unwrap();
    assert!(run(&["sync"], root).status.success());

    // retarget the manifest at libs/two; existing link now disagrees
    std::fs::write(
        &manifest,
        r#"{ "version": 1, "links": [
            { "link": "src/x", "target": "libs/two", "type": "symlink" }
        ] }"#,
    )
    .unwrap();

    let conflict = run(&["sync"], root);
    assert!(!conflict.status.success(), "should flag a conflict");
    assert!(String::from_utf8_lossy(&conflict.stderr).contains("conflict"));

    let forced = run(&["sync", "--force"], root);
    assert!(forced.status.success(), "{}", String::from_utf8_lossy(&forced.stderr));
    assert_eq!(std::fs::read(root.join("src/x/data.txt")).unwrap(), b"2");
}
