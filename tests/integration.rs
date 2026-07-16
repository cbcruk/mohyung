use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn write_file(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn mohyung() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mohyung"))
}

fn make_npm_fixture(root: &Path) -> PathBuf {
    let nm = root.join("node_modules");

    write_file(
        &nm.join("lodash/package.json"),
        r#"{"name":"lodash","version":"4.17.21"}"#,
    );
    write_file(&nm.join("lodash/index.js"), "module.exports = {}");
    write_file(&nm.join("lodash/한글파일.js"), "// multibyte filename");
    write_file(
        &nm.join("@scope/pkg/package.json"),
        r#"{"name":"@scope/pkg","version":"1.0.0"}"#,
    );
    write_file(&nm.join("@scope/pkg/lib/util.js"), "exports.x = 1");
    write_file(&nm.join("@scope/pkg/lib/copy.js"), "exports.x = 1");
    write_file(&nm.join(".package-lock.json"), "{}");
    fs::create_dir_all(nm.join("lodash/empty-dir")).unwrap();

    fs::create_dir_all(nm.join(".bin")).unwrap();
    write_file(&nm.join(".bin/shim.cmd"), "@echo off\r\nnode lodash");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("../lodash/index.js", nm.join(".bin/lodash")).unwrap();
        let script = nm.join("lodash/cli.js");
        write_file(&script, "#!/usr/bin/env node\n");
        make_executable(&script);
    }

    nm
}

fn make_pnpm_fixture(root: &Path) -> PathBuf {
    let nm = root.join("node_modules");

    write_file(
        &nm.join(".pnpm/foo@1.0.0/node_modules/foo/package.json"),
        r#"{"name":"foo","version":"1.0.0"}"#,
    );
    write_file(
        &nm.join(".pnpm/foo@1.0.0/node_modules/foo/index.js"),
        "require('bar')",
    );
    write_file(
        &nm.join(".pnpm/bar@2.0.0/node_modules/bar/package.json"),
        r#"{"name":"bar","version":"2.0.0"}"#,
    );
    write_file(
        &nm.join(".pnpm/bar@2.0.0/node_modules/bar/main.js"),
        "module.exports = 'bar'",
    );
    write_file(&nm.join(".modules.yaml"), "hoistPattern: []");

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            "../../bar@2.0.0/node_modules/bar",
            nm.join(".pnpm/foo@1.0.0/node_modules/bar"),
        )
        .unwrap();
        std::os::unix::fs::symlink(".pnpm/foo@1.0.0/node_modules/foo", nm.join("foo")).unwrap();
    }

    nm
}

fn compare_dir(src: &Path, dst: &Path) {
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let s = entry.path();
        let d = dst.join(entry.file_name());
        let meta = fs::symlink_metadata(&s).unwrap();

        if meta.file_type().is_symlink() {
            let s_target = fs::read_link(&s).unwrap();
            let d_target =
                fs::read_link(&d).unwrap_or_else(|_| panic!("missing symlink: {}", d.display()));
            assert_eq!(
                s_target,
                d_target,
                "symlink target mismatch: {}",
                d.display()
            );
        } else if meta.is_dir() {
            assert!(d.is_dir(), "missing dir: {}", d.display());
            compare_dir(&s, &d);
        } else {
            let s_content = fs::read(&s).unwrap();
            let d_content =
                fs::read(&d).unwrap_or_else(|_| panic!("missing file: {}", d.display()));
            assert_eq!(s_content, d_content, "content mismatch: {}", d.display());

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let s_mode = meta.permissions().mode() & 0o777;
                let d_mode = fs::symlink_metadata(&d).unwrap().permissions().mode() & 0o777;
                assert_eq!(s_mode, d_mode, "mode mismatch: {}", d.display());
            }
        }
    }
}

fn assert_trees_equal(a: &Path, b: &Path) {
    compare_dir(a, b);
    compare_dir(b, a);
}

fn pack_and_unpack(nm: &Path, dir: &Path) -> PathBuf {
    let db = dir.join("nm.db");
    let restored = dir.join("restored");

    mohyung()
        .args([
            "pack",
            "-s",
            nm.to_str().unwrap(),
            "-o",
            db.to_str().unwrap(),
        ])
        .assert()
        .success();
    mohyung()
        .args([
            "unpack",
            "-i",
            db.to_str().unwrap(),
            "-o",
            restored.to_str().unwrap(),
        ])
        .assert()
        .success();

    restored
}

#[test]
fn npm_roundtrip_is_lossless() {
    let dir = TempDir::new().unwrap();
    let nm = make_npm_fixture(dir.path());
    let restored = pack_and_unpack(&nm, dir.path());
    assert_trees_equal(&nm, &restored);
}

#[test]
fn npm_link_roundtrip_is_lossless() {
    let dir = TempDir::new().unwrap();
    let nm = make_npm_fixture(dir.path());
    let db = dir.path().join("nm.db");
    let store = dir.path().join("store");

    mohyung()
        .args([
            "pack",
            "-s",
            nm.to_str().unwrap(),
            "-o",
            db.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Cold restore: blobs are materialized into a fresh store, then linked.
    let restored = dir.path().join("restored-link");
    mohyung()
        .env("MOHYUNG_STORE", store.to_str().unwrap())
        .args([
            "unpack",
            "--link",
            "-i",
            db.to_str().unwrap(),
            "-o",
            restored.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert_trees_equal(&nm, &restored);

    // Warm restore: the store already holds every blob, so it is reused.
    let restored2 = dir.path().join("restored-link-2");
    mohyung()
        .env("MOHYUNG_STORE", store.to_str().unwrap())
        .args([
            "unpack",
            "--link",
            "-i",
            db.to_str().unwrap(),
            "-o",
            restored2.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("reused"));
    assert_trees_equal(&nm, &restored2);
}

#[test]
fn pnpm_roundtrip_is_lossless() {
    let dir = TempDir::new().unwrap();
    let nm = make_pnpm_fixture(dir.path());
    let restored = pack_and_unpack(&nm, dir.path());
    assert_trees_equal(&nm, &restored);

    #[cfg(unix)]
    {
        let through_link = fs::read(restored.join("foo/index.js")).unwrap();
        assert_eq!(through_link, b"require('bar')");
    }
}

#[test]
fn roundtrip_preserves_mtime() {
    let dir = TempDir::new().unwrap();
    let nm = make_npm_fixture(dir.path());
    let restored = pack_and_unpack(&nm, dir.path());

    let original = fs::metadata(nm.join("lodash/index.js"))
        .unwrap()
        .modified()
        .unwrap();
    let recovered = fs::metadata(restored.join("lodash/index.js"))
        .unwrap()
        .modified()
        .unwrap();
    let delta = original
        .duration_since(recovered)
        .unwrap_or_else(|e| e.duration());
    assert!(delta.as_millis() < 1000, "mtime drifted: {:?}", delta);
}

#[test]
fn unpack_refuses_existing_output_without_force() {
    let dir = TempDir::new().unwrap();
    let nm = make_npm_fixture(dir.path());
    let db = dir.path().join("nm.db");

    mohyung()
        .args([
            "pack",
            "-s",
            nm.to_str().unwrap(),
            "-o",
            db.to_str().unwrap(),
        ])
        .assert()
        .success();

    mohyung()
        .args([
            "unpack",
            "-i",
            db.to_str().unwrap(),
            "-o",
            nm.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--force"));

    mohyung()
        .args([
            "unpack",
            "-i",
            db.to_str().unwrap(),
            "-o",
            nm.to_str().unwrap(),
            "-f",
        ])
        .assert()
        .success();
}

#[test]
fn status_detects_modified_deleted_and_added() {
    let dir = TempDir::new().unwrap();
    let nm = make_npm_fixture(dir.path());
    let db = dir.path().join("nm.db");

    mohyung()
        .args([
            "pack",
            "-s",
            nm.to_str().unwrap(),
            "-o",
            db.to_str().unwrap(),
        ])
        .assert()
        .success();

    write_file(&nm.join("lodash/index.js"), "tampered");
    fs::remove_file(nm.join("@scope/pkg/lib/util.js")).unwrap();
    write_file(&nm.join("lodash/new-file.js"), "added later");

    mohyung()
        .args([
            "status",
            "--db",
            db.to_str().unwrap(),
            "-n",
            nm.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("Modified: 1"))
        .stderr(predicate::str::contains("Only in DB: 1"))
        .stderr(predicate::str::contains("Only in FS: 1"));
}

#[test]
fn status_reports_clean_tree() {
    let dir = TempDir::new().unwrap();
    let nm = make_npm_fixture(dir.path());
    let db = dir.path().join("nm.db");

    mohyung()
        .args([
            "pack",
            "-s",
            nm.to_str().unwrap(),
            "-o",
            db.to_str().unwrap(),
        ])
        .assert()
        .success();

    mohyung()
        .args([
            "status",
            "--db",
            db.to_str().unwrap(),
            "-n",
            nm.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("All files match!"));
}

#[test]
fn unpack_rejects_path_traversal() {
    use mohyung::core::store::{self, Store};
    use mohyung::types::PackageInfo;

    let dir = TempDir::new().unwrap();
    let db = dir.path().join("evil.db");

    let mut malicious = Store::create(db.to_str().unwrap()).unwrap();
    malicious
        .transaction(|tx| {
            let pkg_id = store::insert_package(
                tx,
                &PackageInfo {
                    id: None,
                    name: "evil".to_string(),
                    version: "1.0.0".to_string(),
                    path: "evil".to_string(),
                },
            )?;
            store::insert_blob(tx, b"deadbeef", b"payload", 7)?;
            store::insert_file(tx, pkg_id, "../../evil.txt", b"deadbeef", 0o644, 0)?;
            Ok(())
        })
        .unwrap();
    malicious.finalize().unwrap();

    let out = dir.path().join("out");
    mohyung()
        .args([
            "unpack",
            "-i",
            db.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unsafe path"));
    assert!(!dir.path().join("evil.txt").exists());
}

#[test]
fn unpack_rejects_non_database_file() {
    let dir = TempDir::new().unwrap();
    let junk = dir.path().join("junk.db");
    fs::write(&junk, "not sqlite").unwrap();

    let out = dir.path().join("out");
    mohyung()
        .args([
            "unpack",
            "-i",
            junk.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not a mohyung database"));
}
