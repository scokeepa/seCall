---
type: task
status: draft
plan: secall-p15-windows
task_number: 1
title: "NaN 내성 임베딩"
parallel_group: A
depends_on: []
updated_at: 2026-04-09
---

# Task 01: NaN 내성 임베딩

## 문제

`vector.rs:84-106`에서 `embed_batch` 호출이 실패하면 해당 배치의 모든 청크가
`None`으로 남고, `embed_errors > 0`이면 세션 전체가 에러 반환된다.

Ollama bge-m3의 NaN 문제는 배치 내 1개 청크가 원인이지만, Ollama API가 배치 전체를
500 에러로 반환하므로 해당 배치의 모든 청크가 손실된다.

## Changed files

| 파일 | 변경 | 비고 |
|---|---|---|
| `crates/secall-core/src/search/vector.rs:67-145` | 수정 | `index_session()` — 배치 실패 시 개별 재시도 + NaN 필터 + 부분 성공 허용 |

## Change description

### Step 1: NaN 감지 헬퍼 함수 추가

`vector.rs`에 유틸리티 함수 추가:

```rust
/// 임베딩 벡터에 NaN 또는 Inf가 포함되어 있는지 확인
fn has_invalid_values(embedding: &[f32]) -> bool {
    embedding.iter().any(|v| v.is_nan() || v.is_infinite())
}
```

### Step 2: 배치 실패 시 개별 재시도

`index_session()`의 Phase 1 (vector.rs:84-96)을 수정:

```rust
for (batch_idx, text_batch) in texts.chunks(batch_size).enumerate() {
    match self.embedder.embed_batch(text_batch).await {
        Ok(batch_embeddings) => {
            for (i, emb) in batch_embeddings.into_iter().enumerate() {
                let idx = batch_idx * batch_size + i;
                if has_invalid_values(&emb) {
                    tracing::warn!(
                        session_id = %session.id,
                        chunk_idx = idx,
                        "NaN/Inf in embedding, skipping chunk"
                    );
                    embed_errors += 1;
                } else {
                    embeddings[idx] = Some(emb);
                }
            }
        }
        Err(e) => {
            // 배치 실패 → 개별 재시도
            tracing::warn!(
                error = %e,
                batch = batch_idx,
                "batch embed failed, retrying individually"
            );
            for (i, text) in text_batch.iter().enumerate() {
                let idx = batch_idx * batch_size + i;
                match self.embedder.embed(text).await {
                    Ok(emb) if !has_invalid_values(&emb) => {
                        embeddings[idx] = Some(emb);
                    }
                    Ok(_) => {
                        tracing::warn!(
                            session_id = %session.id,
                            chunk_idx = idx,
                            "NaN/Inf in individual embed, skipping"
                        );
                        embed_errors += 1;
                    }
                    Err(e2) => {
                        tracing::warn!(
                            session_id = %session.id,
                            chunk_idx = idx,
                            error = %e2,
                            "individual embed failed, skipping"
                        );
                        embed_errors += 1;
                    }
                }
            }
        }
    }
}
```

### Step 3: 부분 성공 허용

`vector.rs:98-107`의 전체 실패 로직을 부분 성공으로 변경:

```rust
// 기존: embed_errors > 0이면 세션 전체 실패
// 변경: 유효한 임베딩이 하나도 없으면 실패, 부분 성공은 허용

let valid_count = embeddings.iter().filter(|e| e.is_some()).count();
if valid_count == 0 && !chunks.is_empty() {
    return Err(anyhow::anyhow!(
        "session {} embedding completely failed: 0/{} chunks embedded",
        &session.id,
        chunks.len()
    ));
}

if embed_errors > 0 {
    tracing::warn!(
        session_id = %session.id,
        embedded = valid_count,
        skipped = embed_errors,
        total = chunks.len(),
        "partial embedding — some chunks skipped"
    );
}
```

Phase 2 (트랜잭션)는 변경 불필요 — 이미 `if let Some(embedding) = emb_opt`로
`None` 청크를 건너뛰는 로직이 있음 (vector.rs:120-121).

### Step 4: 테스트 추가

```rust
#[test]
fn test_has_invalid_values() {
    assert!(!has_invalid_values(&[1.0, 2.0, 3.0]));
    assert!(has_invalid_values(&[1.0, f32::NAN, 3.0]));
    assert!(has_invalid_values(&[1.0, f32::INFINITY, 3.0]));
    assert!(has_invalid_values(&[f32::NEG_INFINITY]));
    assert!(!has_invalid_values(&[]));
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

# 3. vector 모듈 테스트 집중 확인
cargo test -p secall-core -- search::vector --nocapture

# 4. clippy 경고 없음
cargo clippy --all -- -D warnings
```

## Risks

- **부분 임베딩 세션의 검색 품질**: NaN 청크가 skip되면 해당 턴의 벡터 검색 커버리지가
  줄어듦. 그러나 BM25는 영향 없고, 나머지 청크의 벡터는 정상이므로 전체 누락보다 훨씬 나음.
- **개별 재시도 latency**: 배치 실패 시 N개 개별 호출이 발생. Ollama NaN 문제는
  특정 텍스트에서만 발생하므로 대부분은 개별 호출 성공 → 총 재시도 횟수 적음.
- **ORT/OpenAI에서도 NaN 발생 가능성**: 극히 낮지만 `has_invalid_values` 검사는
  모든 백엔드에 적용되므로 방어적.

## Scope boundary

다음 파일은 이 task에서 수정하지 않음:
- `crates/secall-core/src/search/embedding.rs` — Embedder trait/구현체 변경 없음
- `crates/secall-core/src/search/hybrid.rs` — 검색 로직 변경 없음
- `crates/secall/src/commands/embed.rs` — 임베딩 CLI 변경 없음
- `crates/secall-core/src/search/chunker.rs` — 청킹 로직 변경 없음
