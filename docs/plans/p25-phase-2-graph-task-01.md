---
type: task
plan: p25-phase-2-graph
task_number: 1
status: draft
title: REST /api/daily 엔드포인트 + graph 응답 보강
depends_on: []
parallel_group: null
updated_at: 2026-04-14
---

# Task 01 — REST `/api/daily` 엔드포인트 + graph 응답 보강

## Changed files

1. **`crates/secall-core/src/mcp/server.rs:326`** (수정)
   - `do_daily(date: &str)` 메서드 추가 (기존 `impl SeCallMcpServer` 블록 내)
   - `do_graph_query()` 응답에 노드 라벨/타입 추가

2. **`crates/secall-core/src/mcp/rest.rs:20-86`** (수정)
   - `RestDailyParams` DTO 추가
   - `/api/daily` POST 라우트 + `api_daily` 핸들러 추가
   - `RestGraphParams` DTO 추가 (GraphQueryParams 간소화)

3. **`crates/secall-core/src/store/graph_repo.rs:71`** (수정)
   - `get_node_metadata(node_id) -> Option<(type, label, meta)>` 메서드 추가

## Change description

### Step 1: `graph_repo.rs` — 노드 메타데이터 조회 메서드

```rust
/// 노드의 type, label, meta 조회
pub fn get_node_metadata(&self, node_id: &str) -> Result<Option<(String, String, Option<String>)>> {
    let mut stmt = self.conn().prepare(
        "SELECT type, label, meta FROM graph_nodes WHERE id = ?1"
    )?;
    let result = stmt.query_row([node_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?))
    }).optional()?;
    Ok(result)
}
```

### Step 2: `server.rs` — `do_daily()` 메서드 추가

CLI `commands/log.rs:23-89`의 로직을 참고하되 서버 컨텍스트에 맞게 구현:

1. `db.get_sessions_for_date(date)` 호출
2. 자동화/노이즈 세션 필터링 (turn_count >= 2, session_type != "automated")
3. 노이즈 요약 필터링 ("Analyze the following", "<environment_context>" 등)
4. 프로젝트별 BTreeMap 그룹핑
5. `db.get_topics_for_sessions(session_ids)` 호출
6. JSON 반환:

```json
{
  "date": "2026-04-14",
  "total_sessions": 15,
  "filtered_sessions": 10,
  "topics": ["rust", "async", "graph"],
  "projects": {
    "seCall": {
      "sessions": [
        {
          "session_id": "abc...",
          "summary": "...",
          "turn_count": 5,
          "tools_used": "[Edit, Read]"
        }
      ]
    },
    "tunaFlow": { ... }
  }
}
```

### Step 3: `server.rs` — `do_graph_query()` 응답 보강

현재 응답이 `node_id`, `relation`, `direction`만 반환.
각 결과 노드에 대해 `db.get_node_metadata()`를 호출하여 `type`, `label` 추가:

```json
{
  "node_id": "topic:rust",
  "relation": "discusses_topic",
  "direction": "out",
  "node_type": "topic",
  "label": "rust"
}
```

### Step 4: `rest.rs` — 간소화 DTO + 라우트

```rust
#[derive(Debug, Deserialize)]
struct RestDailyParams {
    date: Option<String>,  // "YYYY-MM-DD", 기본 오늘
}

#[derive(Debug, Deserialize)]
struct RestGraphParams {
    node_id: String,
    depth: Option<usize>,
    relation: Option<String>,
}
```

`rest_router()`에 `.route("/api/daily", post(api_daily))` 추가.
`api_graph` 핸들러를 `RestGraphParams` → `GraphQueryParams` 변환으로 교체.

## Dependencies

- 없음 (첫 번째 Task)
- 기존 `session_repo.rs`의 `get_sessions_for_date`, `get_topics_for_sessions` 재사용

## Verification

```bash
# 1. 컴파일
cargo check 2>&1 | tail -1
# 기대: no errors

# 2. 기존 테스트 통과
cargo test 2>&1 | tail -3
# 기대: 273+ passed, 0 failed

# 3. REST /api/daily 스모크 테스트
cargo build --release 2>&1 | tail -1
./target/release/secall serve --port 8080 &
sleep 2
curl -s -X POST http://127.0.0.1:8080/api/daily \
  -H 'Content-Type: application/json' \
  -d '{"date":"2026-04-05"}' | python3 -m json.tool
# 기대: projects 객체에 프로젝트별 세션 목록 포함

# 4. /api/graph 보강 확인
curl -s -X POST http://127.0.0.1:8080/api/graph \
  -H 'Content-Type: application/json' \
  -d '{"node_id":"project:seCall","depth":1}' | python3 -c "
import sys,json
d=json.load(sys.stdin)
r=d['results'][0] if d['results'] else {}
print('has label:', 'label' in r)
print('has node_type:', 'node_type' in r)
"
# 기대: has label: True, has node_type: True

kill %1 2>/dev/null
```

## Risks

- `do_daily()`는 CLI `log.rs`의 로직을 부분 중복. 공통 추출은 이번 스코프 밖이지만, 필터링 기준(noisy summary 패턴)은 동일하게 유지해야 함
- `get_node_metadata()` 호출이 graph 결과 노드 수만큼 발생 → depth=2+에서 N+1 쿼리. 현재 depth 최대 3이고 결과 수가 제한적이므로 수용 가능

## Scope boundary — 수정 금지

- `crates/secall/src/commands/log.rs` — CLI 명령어 로직은 수정하지 않음
- `crates/secall-core/src/store/session_repo.rs` — 기존 메서드 시그니처 변경 금지
- `obsidian-secall/` — 이 Task에서 플러그인 코드 수정 금지
