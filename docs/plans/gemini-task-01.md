---
type: task
status: draft
plan: gemini
task_number: 1
title: "gemini.rs 파서 재작성"
parallel_group: null
depends_on: []
updated_at: 2026-04-06
---

# Task 01: gemini.rs 파서 재작성

## 문제

현재 serde 모델이 실제 Gemini CLI JSON과 완전히 불일치:

- `GeminiSession`: `id`, `create_time`, `update_time`, `messages[].role` 기대
- 실제: `sessionId`, `startTime`, `lastUpdated`, `messages[].type`
- `GeminiMessage.parts[].text` 기대 → 실제: `content: [{text}]` 또는 `content: "string"`
- `role: "function"` 별도 메시지 + pending queue 기대 → 실제: `toolCalls[]`가 같은 메시지에 call+result 포함
- `thoughts`, `tokens`, `model`, per-message `timestamp` 미처리

## Changed files

| 파일 | 줄 범위 | 변경 |
|---|---|---|
| `crates/secall-core/src/ingest/gemini.rs:29-81` | 수정 | serde 모델 전면 교체 |
| `crates/secall-core/src/ingest/gemini.rs:85-221` | 수정 | `parse_gemini_json()` 함수 재작성 |
| `crates/secall-core/src/ingest/gemini.rs:223-236` | 수정 | `collect_text_parts()` → `extract_content()` 교체 |
| `crates/secall-core/src/ingest/gemini.rs:259-430` | 수정 | 테스트를 실제 JSON 형식으로 업데이트 |

## Change description

### Step 1: serde 모델 교체 (line 29-81 교체)

기존 `GeminiSession`, `GeminiMessage`, `GeminiPart`, `GeminiFunctionCall`, `GeminiFunctionResponse`를 제거하고 실제 구조로 교체:

```rust
// ─── Serde models ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiSession {
    session_id: String,
    #[serde(default)]
    start_time: Option<String>,
    #[serde(default)]
    last_updated: Option<String>,
    #[serde(default)]
    messages: Vec<GeminiMessage>,
    // projectHash, kind — 무시 (serde(default)로 skip)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiMessage {
    #[serde(rename = "type")]
    msg_type: String,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    content: serde_json::Value, // array [{text}] 또는 string
    #[serde(default)]
    thoughts: Option<Vec<GeminiThought>>,
    #[serde(default)]
    tokens: Option<GeminiTokens>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<GeminiToolCall>>,
}

#[derive(Deserialize)]
struct GeminiThought {
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Deserialize)]
struct GeminiTokens {
    #[serde(default)]
    input: u64,
    #[serde(default)]
    output: u64,
    #[serde(default)]
    cached: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiToolCall {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    args: serde_json::Value,
    #[serde(default)]
    result: Option<Vec<GeminiToolResult>>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiToolResult {
    #[serde(default)]
    function_response: Option<GeminiFunctionResponse>,
}

#[derive(Deserialize)]
struct GeminiFunctionResponse {
    #[serde(default)]
    response: serde_json::Value,
}
```

**설계 이유**:
- `content`를 `serde_json::Value`로 받아 string/array 양쪽 처리
- `GeminiToolCall`에 `result`가 포함 — pending queue 불필요
- `thoughts`는 `Vec<GeminiThought>`으로 직접 매핑
- `rename_all = "camelCase"`로 `toolCalls` → `tool_calls`, `sessionId` → `session_id` 자동 변환

### Step 2: parse_gemini_json() 재작성 (line 85-221 교체)

```rust
pub fn parse_gemini_json(path: &Path) -> Result<Session> {
    let metadata = std::fs::metadata(path)?;
    if metadata.len() > 100 * 1024 * 1024 {
        eprintln!(
            "warn: gemini session file is large ({} MB): {}",
            metadata.len() / 1024 / 1024,
            path.display()
        );
    }

    let raw = std::fs::read_to_string(path)?;
    let gs: GeminiSession = serde_json::from_str(&raw)
        .map_err(|e| anyhow!("failed to parse gemini session {}: {e}", path.display()))?;

    let project = extract_project_id(path);

    let mut turns: Vec<Turn> = Vec::new();
    let mut turn_idx: u32 = 0;
    let mut session_model: Option<String> = None;

    for msg in &gs.messages {
        // 턴 타임스탬프
        let ts = msg.timestamp.as_deref()
            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
            .map(|dt| dt.with_timezone(&Utc));

        match msg.msg_type.as_str() {
            "user" => {
                let content = extract_content(&msg.content);
                if !content.is_empty() {
                    turns.push(Turn {
                        index: turn_idx,
                        role: Role::User,
                        timestamp: ts,
                        content,
                        actions: Vec::new(),
                        tokens: None,
                        thinking: None,
                        is_sidechain: false,
                    });
                    turn_idx += 1;
                }
            }
            "gemini" => {
                let content = extract_content(&msg.content);

                // model — 첫 번째 gemini 메시지에서 추출
                if session_model.is_none() {
                    session_model = msg.model.clone();
                }

                // thinking — thoughts[].description 결합
                let thinking = msg.thoughts.as_ref().and_then(|thoughts| {
                    let text: Vec<String> = thoughts.iter()
                        .filter_map(|t| t.description.clone())
                        .collect();
                    if text.is_empty() { None } else { Some(text.join("\n\n")) }
                });

                // tokens
                let tokens = msg.tokens.as_ref().map(|t| TokenUsage {
                    input: t.input,
                    output: t.output,
                    cached: t.cached,
                });

                // toolCalls → actions
                let mut actions = Vec::new();
                if let Some(tool_calls) = &msg.tool_calls {
                    for tc in tool_calls {
                        let name = tc.name.clone().unwrap_or_else(|| "unknown".to_string());
                        let input_summary = tc.args.to_string();

                        // result[0].functionResponse.response에서 output 추출
                        let output_summary = tc.result.as_ref()
                            .and_then(|results| results.first())
                            .and_then(|r| r.function_response.as_ref())
                            .map(|fr| {
                                // response.output 또는 response.error 추출
                                fr.response.get("output")
                                    .or_else(|| fr.response.get("error"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string()
                            })
                            .unwrap_or_default();

                        actions.push(Action::ToolUse {
                            name,
                            input_summary,
                            output_summary,
                            tool_use_id: None,
                        });
                    }
                }

                turns.push(Turn {
                    index: turn_idx,
                    role: Role::Assistant,
                    timestamp: ts,
                    content,
                    actions,
                    tokens,
                    thinking,
                    is_sidechain: false,
                });
                turn_idx += 1;
            }
            // "info" 등 → skip
            _ => {}
        }
    }

    if turns.is_empty() {
        return Err(anyhow!(
            "gemini session has no parseable turns: {}",
            path.display()
        ));
    }

    use chrono::DateTime;

    let start_time = gs.start_time.as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);

    let end_time = gs.last_updated.as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc));

    Ok(Session {
        id: gs.session_id,
        agent: AgentKind::GeminiCli,
        model: session_model,
        project,
        cwd: None,
        git_branch: None,
        start_time,
        end_time,
        turns,
        total_tokens: Default::default(),
    })
}
```

### Step 3: extract_content() 교체 (line 223-236 교체)

기존 `collect_text_parts()`를 `extract_content()`로 교체. content가 string 또는 array 두 형태를 모두 처리:

```rust
/// content가 string이면 그대로, array이면 [{text: "..."}]에서 text 추출
fn extract_content(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|item| item.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}
```

### Step 4: extract_project_id() 유지 (line 238-257)

변경 없음. 경로 기반 프로젝트 ID 추출은 그대로 유지. `projectHash`는 SHA256 해시라 프로젝트명으로 부적합.

### Step 5: 테스트 업데이트 (line 259-430 교체)

모든 테스트를 실제 JSON 형식으로 교체:

1. **test_gemini_parse_basic** — 새 필드명(`sessionId`, `startTime`, `lastUpdated`, `type: "user"/"gemini"`)
2. **test_gemini_tool_calls** — `toolCalls[]` 내부에 call+result 포함 구조
3. **test_gemini_thinking** — `thoughts` 배열 → Turn.thinking 매핑
4. **test_gemini_tokens** — `tokens` → Turn.tokens (TokenUsage) 매핑
5. **test_gemini_model** — 첫 번째 gemini 메시지의 `model` → Session.model
6. **test_gemini_string_content** — `content: "string"` (non-array) 처리
7. **test_gemini_info_skip** — `type: "info"` 메시지가 턴에 포함되지 않음
8. **test_gemini_timestamps_parsed** — `startTime`/`lastUpdated` → start_time/end_time
9. **test_gemini_detect_path** — 변경 없음 (경로 판별 동일)
10. **test_gemini_project_extraction** — 변경 없음 (경로 기반)

테스트 JSON 예시:

```json
// 기본 세션
{
  "sessionId": "test-uuid",
  "startTime": "2026-04-05T10:00:00Z",
  "lastUpdated": "2026-04-05T10:30:00Z",
  "kind": "main",
  "projectHash": "abc123",
  "messages": [
    {"id":"m1","timestamp":"2026-04-05T10:00:01Z","type":"user","content":[{"text":"검색 기능 구현해줘"}]},
    {"id":"m2","timestamp":"2026-04-05T10:00:02Z","type":"gemini","content":"네, 구현하겠습니다.","model":"gemini-3.1-pro","tokens":{"input":100,"output":50,"cached":0,"thoughts":0,"tool":0,"total":150}}
  ]
}

// toolCalls 포함
{"id":"m3","timestamp":"2026-04-05T10:00:03Z","type":"gemini","content":"","toolCalls":[
  {"name":"read_file","args":{"file_path":"main.rs"},"result":[{"functionResponse":{"name":"read_file","response":{"output":"fn main() {}"}}}],"status":"success"}
],"tokens":{"input":200,"output":10,"cached":0,"thoughts":0,"tool":0,"total":210}}

// thoughts 포함
{"id":"m4","timestamp":"2026-04-05T10:00:04Z","type":"gemini","content":"결과입니다.","thoughts":[
  {"subject":"Analyzing","description":"코드를 분석하고 있습니다.","timestamp":"2026-04-05T10:00:03Z"},
  {"subject":"Planning","description":"구현 계획을 세우고 있습니다.","timestamp":"2026-04-05T10:00:04Z"}
],"tokens":{"input":100,"output":30,"cached":0,"thoughts":50,"tool":0,"total":180}}

// info (skip)
{"id":"m5","timestamp":"2026-04-05T10:00:05Z","type":"info","content":"Request cancelled."}
```

## Dependencies

- 없음. 다른 task에 의존하지 않음.
- 외부 crate 추가 없음 (기존 serde, serde_json, chrono, anyhow 사용).

## Verification

```bash
# 1. 컴파일 확인
cargo check -p secall-core

# 2. Gemini 파서 유닛 테스트
cargo test -p secall-core gemini

# 3. 전체 테스트 회귀 없음
cargo test

# 4. 실제 Gemini 세션 파싱 (0 errors 확인)
# 주의: ingest 커맨드에 디렉토리를 전달하면 find_claude_sessions()만 호출되어
# Gemini 파일을 찾지 못한다 (ingest.rs pre-existing 동작, 이 task scope 외).
# 개별 파일 경로를 직접 전달해야 정상 파싱이 확인된다.
cargo run -p secall -- ingest ~/.gemini/tmp/repos/chats/session-2026-04-04T02-49-a6fdff5d.json 2>&1 | tail -1
# 기대 출력: Summary: 1 ingested, 0 skipped (duplicate), 0 errors
#
# [참고] 디렉토리 전달 시 실제 출력:
# cargo run -p secall -- ingest ~/.gemini/tmp 2>&1 | tail -1
# → "No sessions to ingest."  (파서 오류가 아닌 탐색 경로 미매핑 문제)
```

> **[Developer 필수]** subtask-done 시그널 전에 위 명령의 실행 결과를 result 문서에 기록하세요. 형식: `✅ 명령 — exit 0` 또는 `❌ 명령 — 에러 내용 (사유)`. 검증 증빙 미제출 시 리뷰에서 conditional 처리됩니다.

## Risks

- **Gemini CLI 버전 차이**: v3.1+ 기준 분석. 이전 버전의 세션 파일이 다른 키를 사용할 수 있음. `serde(default)` + `serde_json::Value`로 방어하므로 크래시는 없으나, 구버전 세션이 0 turns로 에러 처리될 수 있음.
- **content 타입 혼재**: `serde_json::Value`로 받고 `extract_content()`에서 string/array 분기 처리. `null`이면 빈 문자열 반환.
- **toolCalls.result 배열 복수 항목**: 분석 샘플에서는 항상 단일 항목이었으나, `results.first()`로 첫 번째만 사용. 복수 결과가 있으면 첫 번째만 반영됨 — 실용적으로 문제 없음.
- **대용량 response.output**: `functionResponse.response.output`이 매우 클 수 있음. vault 마크다운 렌더링 시 `TOOL_OUTPUT_MAX_CHARS` truncation이 적용되므로 vault 크기 문제는 없음.
- **thoughts 비암호화**: Codex와 달리 Gemini thoughts는 평문. 세션 마크다운에 thinking 블록으로 노출됨 — 의도된 동작.

## Scope boundary

다음 파일은 이 task에서 수정하지 않음:

- `crates/secall-core/src/ingest/claude.rs` — Claude 파서
- `crates/secall-core/src/ingest/codex.rs` — Codex 파서
- `crates/secall-core/src/ingest/types.rs` — Session/Turn/Action/TokenUsage 타입
- `crates/secall-core/src/ingest/markdown.rs` — vault 마크다운 렌더링
- `crates/secall-core/src/ingest/detect.rs` — 세션 파일 탐색
- `crates/secall/src/commands/ingest.rs` — ingest CLI 커맨드
