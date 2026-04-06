use std::process::Command;

fn secall_cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_secall"))
}

#[test]
fn test_cli_help() {
    let output = secall_cmd().arg("--help").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("secall"));
}

#[test]
fn test_cli_version() {
    let output = secall_cmd().arg("--version").output().unwrap();
    assert!(output.status.success());
}

#[test]
fn test_cli_status_without_db() {
    // DB가 없는 상태에서 status 실행 → 패닉하지 않아야 함
    let output = secall_cmd()
        .arg("status")
        .env("SECALL_DB_PATH", "/tmp/secall-test-nonexistent.db")
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&output.stderr).contains("panic"));
}

#[test]
fn test_cli_lint_without_db() {
    let output = secall_cmd()
        .arg("lint")
        .env("SECALL_DB_PATH", "/tmp/secall-test-nonexistent.db")
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&output.stderr).contains("panic"));
}
