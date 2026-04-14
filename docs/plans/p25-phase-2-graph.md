---
type: plan
status: draft
updated_at: 2026-04-14
slug: p25-phase-2-graph
---

# P25 Phase 2 — 데일리 노트 자동 생성 + Graph 탐색 뷰

## Background

Phase 0-1에서 REST API(`secall serve`) + Obsidian 플러그인 MVP(검색/세션뷰/상태바)를 구현했다.
Phase 2는 이 기반 위에 두 가지 핵심 기능을 추가한다:

1. **데일리 노트 자동 생성** — 특정 날짜의 세션을 프로젝트별로 그룹핑하여 Obsidian 노트로 생성
2. **Graph 탐색 뷰** — 노드를 클릭하며 관계를 확장하는 인터랙티브 트리 뷰

## 기존 인프라 (재사용 가능)

| 모듈 | 위치 | 설명 |
|------|------|------|
| `get_sessions_for_date(date)` | `session_repo.rs:552` | 날짜별 세션 조회 (id, project, summary, turn_count, tools_used, session_type) |
| `get_topics_for_sessions(ids)` | `session_repo.rs:590` | 세션들의 discusses_topic 엣지 조회 |
| `generate_template()` | `commands/log.rs:189` | 프로젝트별 그룹핑 + 토픽 포함 마크다운 템플릿 생성 |
| `do_graph_query()` | `mcp/server.rs:326` | BFS 그래프 탐색 (depth 1~3, relation 필터) |
| `get_neighbors()` | `graph_repo.rs:71` | 양방향 이웃 노드 조회 |
| REST `/api/graph` | `mcp/rest.rs:148` | 그래프 쿼리 엔드포인트 (이미 존재) |

## Subtasks

| # | 제목 | 핵심 파일 | depends_on | parallel_group |
|---|------|-----------|------------|----------------|
| 01 | REST `/api/daily` 엔드포인트 + graph 응답 보강 | `server.rs`, `rest.rs`, `graph_repo.rs` | 없음 | — |
| 02 | Obsidian 데일리 노트 뷰 + 노트 생성 | `daily-view.ts` (신규), `api.ts`, `settings.ts`, `main.ts` | 01 | views |
| 03 | Obsidian Graph 탐색 뷰 | `graph-view.ts` (신규), `api.ts`, `main.ts`, `styles.css` | 01 | views |
| 04 | 통합 테스트 + 스모크 검증 | curl 테스트, cargo test, tsc | 02, 03 | — |

Task 02와 03은 01 완료 후 **병렬 실행 가능**.

## 비용/위험

| 위험 | 대응 |
|------|------|
| 데일리 노트 중복 생성 | 파일 존재 여부 체크 → 사용자에게 overwrite/skip 선택 |
| Graph 뷰 노드 폭발 | depth 기본 1, 최대 2, lazy expand |
| Obsidian daily notes 플러그인 충돌 | 별도 폴더 `seCall/daily/` 사용, 기존 daily notes 미접촉 |
| `do_daily` 로직이 CLI `log`과 중복 | `log.rs`의 필터링/그룹핑 로직을 server.rs에서 재구현 (공통 추출은 Phase 3에서 고려) |

## 테스트 기준선 (2026-04-14)

```
secall-core: 253 passed, 0 failed, 10 ignored
secall:      16 passed (+ 4 integration)
총:          273 passed
```
