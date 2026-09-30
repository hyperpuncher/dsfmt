use std::io::Write;
use std::process::{Command, Output, Stdio};

const INPUT: &str = "<div data-text=\" $count \" />";
const FORMATTED: &str = "<div data-text=\"$count\" />";

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dsfmt"))
}

fn stdin(args: &[&str], input: &str) -> Output {
    let mut child = command()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn stdin_check_reports_changes_without_printing_source() {
    let changed = stdin(&["--check"], INPUT);
    assert_eq!(changed.status.code(), Some(1));
    assert!(changed.stdout.is_empty());
    assert!(
        String::from_utf8(changed.stderr)
            .unwrap()
            .contains("stdin would be reformatted")
    );
    let unchanged = stdin(&["--check"], FORMATTED);
    assert!(unchanged.status.success());
    assert!(unchanged.stdout.is_empty());
    assert_eq!(stdin(&[], INPUT).stdout, FORMATTED.as_bytes());
}

#[test]
fn stdin_filename_selects_the_host_without_guessing_from_expression_text() {
    // The arrow would trigger TSX guessing, but the modifier is an HTML name.
    let input = "<button data-on:click__debounce.500ms=\"() =>  $count++\" />";
    let output = stdin(&["--stdin-filepath", "snippet.html"], input);
    assert!(output.status.success());
    assert_eq!(
        output.stdout,
        b"<button data-on:click__debounce.500ms=\"() => $count++\" />"
    );
    let output = stdin(
        &["--stdin-filepath", "snippet.tsx"],
        "<div data-text={`$count+1`} />",
    );
    assert!(output.status.success());
    assert_eq!(output.stdout, b"<div data-text={`$count + 1`} />");
}

#[test]
fn invalid_widths_and_conflicting_modes_are_rejected() {
    for args in [
        ["--tab-width", "0"],
        ["--line-width", "0"],
        ["--write", "--check"],
    ] {
        assert_eq!(stdin(&args, INPUT).status.code(), Some(2));
    }
}

#[test]
fn check_reports_all_files_and_does_not_write() {
    let directory = tempfile::tempdir().unwrap();
    let paths: Vec<_> = ["a.html", "b.html"]
        .map(|name| directory.path().join(name))
        .into();
    for path in &paths {
        std::fs::write(path, INPUT).unwrap();
    }
    let output = command().arg("--check").args(&paths).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let errors = String::from_utf8(output.stderr).unwrap();
    for path in &paths {
        assert!(errors.contains(path.file_name().unwrap().to_str().unwrap()));
        assert_eq!(std::fs::read_to_string(path).unwrap(), INPUT);
    }
}

#[test]
fn directory_walk_handles_blade_htm_and_case_insensitive_extensions() {
    let directory = tempfile::tempdir().unwrap();
    for name in [
        "page.blade.php",
        "page.htm",
        "page.HTML",
        "page.TSX",
        "ignored.html",
        "other.txt",
    ] {
        std::fs::write(directory.path().join(name), INPUT).unwrap();
    }
    std::fs::write(directory.path().join(".ignore"), "ignored.html\n").unwrap();
    assert!(
        command()
            .arg("--write")
            .arg(directory.path())
            .status()
            .unwrap()
            .success()
    );
    for name in ["page.blade.php", "page.htm", "page.HTML", "page.TSX"] {
        assert_eq!(
            std::fs::read_to_string(directory.path().join(name)).unwrap(),
            FORMATTED
        );
    }
    for name in ["ignored.html", "other.txt"] {
        assert_eq!(
            std::fs::read_to_string(directory.path().join(name)).unwrap(),
            INPUT
        );
    }
}

#[test]
fn read_failures_do_not_pass_check() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("broken.html");
    std::fs::write(&path, [0xff, 0xfe]).unwrap();
    assert_eq!(
        command()
            .arg("--check")
            .arg(&path)
            .output()
            .unwrap()
            .status
            .code(),
        Some(1)
    );
    assert_eq!(
        command()
            .arg(directory.path().join("missing.html"))
            .output()
            .unwrap()
            .status
            .code(),
        Some(1)
    );
}

#[test]
fn read_only_write_failure_keeps_original_and_exits_nonzero() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("readonly.html");
    std::fs::write(&path, INPUT).unwrap();
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions).unwrap();
    assert_eq!(
        command()
            .arg("--write")
            .arg(&path)
            .output()
            .unwrap()
            .status
            .code(),
        Some(1)
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), INPUT);
}

#[cfg(unix)]
#[test]
fn atomic_writes_preserve_permissions_and_follow_symlinks() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("target.html");
    let link = directory.path().join("link.html");
    std::fs::write(&target, INPUT).unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).unwrap();
    symlink(&target, &link).unwrap();
    assert!(
        command()
            .arg("--write")
            .arg(&link)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(std::fs::read_to_string(&target).unwrap(), FORMATTED);
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 2);
}

#[cfg(unix)]
#[test]
fn non_utf8_paths_are_not_replaced_with_lossy_names() {
    use std::os::unix::ffi::OsStringExt;
    let directory = tempfile::tempdir().unwrap();
    let path = directory
        .path()
        .join(std::ffi::OsString::from_vec(b"page\xff.html".to_vec()));
    std::fs::write(&path, INPUT).unwrap();
    assert!(
        command()
            .arg("--write")
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), FORMATTED);
}
