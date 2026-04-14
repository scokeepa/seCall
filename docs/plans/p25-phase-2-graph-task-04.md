---
type: task
plan: p25-phase-2-graph
task_number: 4
status: draft
title: 통합 테스트 + 스모크 검증
depends_on: [2, 3]
parallel_group: null
updated_at: 2026-04-14
---

# Task 04 — 통합 테스트 + 스모크 검증

## Changed files

변경 없음 — 검증 전용 Task.

단, 검증 과정에서 발견된 버그는 해당 파일에서 수정 가능:
- `crates/secall-core/src/mcp/server.rs`
- `crates/secall-core/src/mcp/rest.rs`
- `obsidian-secall/src/*.ts`
- `obsidian-secall/styles.css`

## Change description

### Step 1: Rust 빌드 + 테스트

1. `cargo check` — 컴파일 에러 없음 확인
2. `cargo test` — 기존 273+ 테스트 통과 확인 (새로운 테스트 추가 시 포함)
3. `cargo clippy` — 경고 확인 (수정은 선택사항)

### Step 2: REST API 스모크 테스트

서버 기동 후 모든 엔드포인트 순차 검증:

1. `GET /api/status` — 200 + sessions/vectors JSON
2. `POST /api/recall` — `{"query":"test","limit":2}` → results 배열
3. `POST /api/get` — `{"session_id":"...", "full":true}` → content 포함
4. `POST /api/daily` — `{"date":"2026-04-05"}` → projects 객체, topics 배열
5. `POST /api/daily` — `{}` (날짜 없음 = 오늘) → 정상 응답 또는 빈 결과
6. `POST /api/graph` — `{"node_id":"project:seCall","depth":1}` → label, node_type 포함
7. CORS preflight — `OPTIONS /api/daily` + `Origin: app://obsidian` → `access-control-allow-origin: *`

### Step 3: Obsidian 플러그인 빌드

1. `npx tsc --noEmit` — TypeScript 타입 에러 없음
2. `node esbuild.config.mjs production` — main.js 번들 생성

### Step 4: Obsidian 플러그인 수동 검증 체크리스트

```
# Manual: Obsidian에서 플러그인 로드 후 검증
# (secall serve --port 8080이 실행 중인 상태에서)

1. [ ] Cmd+P → "seCall: Search" → 검색 실행 → 결과에 "Graph" 버튼 표시
2. [ ] Graph 버튼 클릭 → Graph 탐색 뷰 열림, 노드 목록 표시
3. [ ] Graph 뷰에서 노드 클릭 → 해당 노드로 재탐색 (breadcrumb 업데이트)
4. [ ] session 노드 클릭 → SessionView로 세션 내용 표시
5. [ ] Cmd+P → "seCall: Daily Note" → 데일리 뷰 열림
6. [ ] 데일리 뷰: 날짜 이동 (< >) → API 호출, 프로젝트별 세션 표시
7. [ ] "노트 생성" 버튼 → seCall/daily/YYYY-MM-DD.md 파일 생성됨
8. [ ] 이미 존재하는 날짜에 노트 생성 → 덮어쓰기 Notice 표시
9. [ ] 하단 상태바: "seCall: N sessions, vectors ✓" 정상 표시
```

## Dependencies

- **Task 01, 02, 03** 모두 완료되어야 함

## Verification

```bash
# 1. Rust 컴파일
cargo check 2>&1 | tail -1
# 기대: no errors

# 2. Rust 테스트
cargo test 2>&1 | tail -3
# 기대: 273+ passed, 0 failed

# 3. TypeScript 타입 체크
cd obsidian-secall && npx tsc --noEmit 2>&1 | tail -5
# 기대: no errors

# 4. esbuild 번들
cd obsidian-secall && node esbuild.config.mjs production 2>&1
# 기대: 성공

# 5. REST 전체 스모크
cargo build --release 2>&1 | tail -1
./target/release/secall serve --port 8080 &
sleep 2

echo "--- status ---"
curl -sf http://127.0.0.1:8080/api/status | python3 -c "import sys,json; d=json.load(sys.stdin); print(f'sessions={d[\"sessions\"]}')"

echo "--- recall ---"
curl -sf -X POST http://127.0.0.1:8080/api/recall \
  -H 'Content-Type: application/json' \
  -d '{"query":"test","limit":1}' | python3 -c "import sys,json; d=json.load(sys.stdin); print(f'count={d[\"count\"]}')"

echo "--- daily ---"
curl -sf -X POST http://127.0.0.1:8080/api/daily \
  -H 'Content-Type: application/json' \
  -d '{"date":"2026-04-05"}' | python3 -c "import sys,json; d=json.load(sys.stdin); print(f'projects={list(d.get(\"projects\",{}).keys())}')"

echo "--- graph ---"
curl -sf -X POST http://127.0.0.1:8080/api/graph \
  -H 'Content-Type: application/json' \
  -d '{"node_id":"project:seCall","depth":1}' | python3 -c "import sys,json; d=json.load(sys.stdin); print(f'count={d[\"count\"]}, has_label={\"label\" in d[\"results\"][0] if d[\"results\"] else \"no results\"}')"

echo "--- CORS ---"
curl -sf -I -X OPTIONS http://127.0.0.1:8080/api/daily \
  -H 'Origin: app://obsidian' \
  -H 'Access-Control-Request-Method: POST' 2>&1 | grep -i "access-control-allow-origin"

kill %1 2>/dev/null
echo "--- All smoke tests done ---"
# 기대: 모든 엔드포인트 정상 응답
```

## Risks

- 서버가 로컬에서 실행 중이어야 스모크 테스트 가능 — CI에서는 서버 기동이 별도 필요
- Obsidian 수동 검증은 자동화 불가 — 체크리스트로 대체

## Scope boundary — 수정 금지

- 버그 수정 외의 신규 기능 추가 금지
- `docs/plans/` — 플랜 문서 수정 금지
- `crates/secall/src/commands/` — serve.rs 외 CLI 명령어 수정 금지
