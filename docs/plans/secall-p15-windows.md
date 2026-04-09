---
type: plan
status: draft
updated_at: 2026-04-09
version: 1
---

# seCall P15 — Windows 런타임 수정

## Description

Windows 테스트에서 발견된 2개 이슈를 수정한다:

**#5 Ollama bge-m3 NaN**: Ollama의 bge-m3가 특정 짧은 영문 문장에서 NaN을 반환.
현재 `vector.rs:100-106`에서 배치 하나라도 실패하면 세션 전체 벡터가 누락된다.
부분 성공을 허용하고 NaN 벡터를 필터링하여 유효한 청크는 정상 저장한다.

**#6 command_exists uses `which`**: `wiki.rs:147`과 `query_expand.rs:55`에서 Unix 전용
`which` 명령을 하드코딩. Windows에는 `which`가 없어 claude CLI 감지가 항상 실패한다.
크로스플랫폼 감지로 통합한다.

## Expected Outcome

- NaN을 반환하는 청크만 skip, 나머지 청크는 정상 임베딩/저장
- embed 로그에 NaN skip 사유와 청크 정보 출력
- Windows에서 `secall wiki update`, `secall sync` (wiki 단계), `query --expand`가 정상 동작
- 기존 테스트 전부 통과 + 새 테스트 추가

## Subtasks

| # | Title | 공수 | parallel_group | depends_on |
|---|-------|------|---------------|------------|
| 01 | NaN 내성 임베딩 | Small | A | — |
| 02 | 크로스플랫폼 command_exists | Small | A | — |

두 작업은 완전히 독립적이며 병렬 수행 가능.

## Constraints

- `Embedder` trait 인터페이스 변경 없음
- ORT/OpenAI 백엔드에 영향 없음
- `command_exists` 통합 시 기존 호출자 시그니처 유지

## Non-goals

- Ollama upstream NaN 버그 수정 (upstream 이슈)
- 새 임베딩 모델 지원 추가
- Windows 전용 테스트 CI 추가 (P13에서 이미 구축)
- CLI에 `--skip-nan` 같은 옵션 추가 (항상 활성화)
