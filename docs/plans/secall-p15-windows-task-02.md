---
type: task
status: draft
plan: secall-p15-windows
task_number: 2
title: "크로스플랫폼 command_exists"
parallel_group: A
depends_on: []
updated_at: 2026-04-09
---

# Task 02: 크로스플랫폼 command_exists

## 문제

`command_exists()`가 2곳에 중복 정의되어 있으며, 둘 다 Unix 전용 `which`를 사용:

- `crates/secall/src/commands/wiki.rs:147-153`
- `crates/secall-core/src/search/query_expand.rs:55-61`

Windows에는 `which`가 없어 항상 `false` 반환 → claude CLI 감지 실패 →
`wiki update`, `sync` wiki 단계, `query --expand` 모두 동작 불가.

## Changed files

| 파일 | 변경 | 비고 |
|---|---|---|
| `crates/secall-core/src/lib.rs` (또는 적절한 공통 모듈) | 수정 | `command_exists()` 공통 함수 추가 |
| `crates/secall/src/commands/wiki.rs:147-153` | 수정 | 로컬 `command_exists` 삭제, 공통 함수 사용 |
| `crates/secall-core/src/search/query_expand.rs:55-61` | 수정 | 로컬 `command_exists` 삭제, 공통 함수 사용 |
| `crates/secall/src/commands/sync.rs:88` | 확인 | `wiki::command_exists` 호출 → 경로 변경 반영 |

## Change description

### Step 1: 공통 command_exists 함수 작성

`secall-core`에 유틸리티 모듈이 없으면 기존 적절한 위치에 추가.
`crates/secall-core/src/util.rs` (신규) 또는 `crates/secall-core/src/lib.rs`에:

```rust
/// 크로스플랫폼 명령어 존재 확인
pub fn command_exists(cmd: &str) -> bool {
    #[cfg(target_os = "windows")]
    let check = std::process::Command::new("where.exe")
        .arg(cmd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    #[cfg(not(target_os = "windows"))]
    let check = std::process::Command::new("which")
        .arg(cmd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    check
}
```

**설계 결정:**
- `where.exe`는 Windows에서 `which`에 대응하는 네이티브 명령어
- `stdout/stderr`를 `null()`로 리다이렉트하여 불필요한 출력 억제
- `output()` 대신 `status()`로 exit code만 확인 — 더 가벼움

### Step 2: wiki.rs에서 로컬 함수 삭제

`crates/secall/src/commands/wiki.rs:147-153`:

```rust
// 삭제:
// pub fn command_exists(cmd: &str) -> bool {
//     std::process::Command::new("which")
//         .arg(cmd)
//         .output()
//         .map(|o| o.status.success())
//         .unwrap_or(false)
// }
```

`wiki.rs:33`의 호출을 공통 함수로 교체:
```rust
// 변경 전
if !command_exists("claude") {
// 변경 후
if !secall_core::command_exists("claude") {
```

### Step 3: query_expand.rs에서 로컬 함수 삭제

`crates/secall-core/src/search/query_expand.rs:55-61`:

```rust
// 삭제:
// fn command_exists(cmd: &str) -> bool { ... }
```

`query_expand.rs:19`의 호출을 공통 함수로 교체:
```rust
// 변경 전
if !command_exists("claude") {
// 변경 후
if !crate::command_exists("claude") {
```

### Step 4: sync.rs 호출 경로 확인

`crates/secall/src/commands/sync.rs:88`:
```rust
if !wiki::command_exists("claude") {
```

`wiki.rs`에서 `command_exists`를 삭제하므로 이 호출도 변경:
```rust
if !secall_core::command_exists("claude") {
```

### Step 5: 테스트 수정

`query_expand.rs`의 기존 테스트 (71행, 80-81행)가 로컬 `command_exists`를 참조:

```rust
// 기존
if command_exists("claude") {
assert!(!command_exists("__nonexistent_command_xyz__"));

// 변경
if crate::command_exists("claude") {
assert!(!crate::command_exists("__nonexistent_command_xyz__"));
```

새 테스트 추가 (공통 함수):

```rust
#[test]
fn test_command_exists_known() {
    // git은 개발 환경에서 반드시 존재
    assert!(command_exists("git"));
}

#[test]
fn test_command_exists_unknown() {
    assert!(!command_exists("__nonexistent_command_xyz_12345__"));
}
```

## Dependencies

- 없음

## Verification

```bash
# 1. 컴파일 확인
cargo check --all

# 2. 전체 테스트 통과
cargo test --all

# 3. query_expand 테스트 (command_exists 참조 변경)
cargo test -p secall-core -- search::query_expand --nocapture

# 4. clippy 경고 없음
cargo clippy --all -- -D warnings

# 5. 중복 정의가 남아있지 않은지 확인
grep -rn "fn command_exists" crates/ | grep -v "test" | wc -l
# 기대값: 1 (공통 함수 1개만)
```

## Risks

- **`where.exe` 동작 차이**: `where.exe`는 매칭되는 모든 경로를 출력하고,
  찾지 못하면 exit code 1을 반환. `which`와 동일한 성공/실패 시맨틱이므로 호환.
- **PATH 환경 변수 차이**: Windows에서는 `.exe` 확장자 자동 해석.
  `where.exe claude`는 `claude.exe`를 찾아줌 — 추가 처리 불필요.
- **sync.rs 호출 경로 누락**: `wiki::command_exists`를 참조하는 곳이
  `sync.rs:88` 1곳 — 반드시 함께 변경해야 컴파일됨.

## Scope boundary

다음 파일은 이 task에서 수정하지 않음:
- `crates/secall-core/src/search/embedding.rs` — Task 01 영역
- `crates/secall-core/src/search/vector.rs` — Task 01 영역
- `crates/secall-core/src/search/hybrid.rs` — P14 영역
- `crates/secall/src/commands/embed.rs` — 임베딩 CLI 변경 없음
