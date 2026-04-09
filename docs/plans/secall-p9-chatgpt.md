---
type: plan
status: draft
updated_at: 2026-04-07
version: 1
---

# seCall P9 — ChatGPT 파서

## Description

OpenAI ChatGPT export (conversations.json) 파서를 추가하여 에이전트 커버리지를 확대합니다.
ChatGPT "Export data" 기능으로 받는 ZIP에서 conversations.json을 추출하고 통합 Session 모델로 변환합니다.

## Expected Outcome

- `AgentKind::ChatGpt` 추가
- ChatGPT export ZIP/JSON 자동 감지 + 파싱 → vault MD 생성
- `secall ingest ~/Downloads/chatgpt-export.zip` 한 줄로 동작
- 기존 검색/위키/MCP에서 ChatGPT 세션도 통합 검색 가능

## Subtasks

| # | Task | 파일 | depends_on | parallel_group |
|---|---|---|---|---|
| 01 | ChatGPT export 포맷 분석 | (분석 문서만, 코드 없음) | - | A |
| 02 | ChatGPT 파서 구현 | chatgpt.rs, types.rs, detect.rs, mod.rs | 01 | B |
| 03 | 테스트 + E2E 검증 | chatgpt.rs (tests), detect.rs (tests) | 02 | C |

## ChatGPT Export 포맷 (실제 export 분석 결과)

ChatGPT "Settings → Data controls → Export data"로 받는 ZIP:

```
chatgpt-export.zip
├── conversations.json    ← 핵심
├── chat.html             ← HTML 렌더링 (무시)
├── message_feedback.json ← 피드백 (무시)
├── model_comparisons.json ← (무시)
├── shared_conversations.json ← (무시)
└── user.json             ← 사용자 정보 (무시)
```

실제 샘플 ZIP:

- `desktop_conversation/chatGPT/6e0b123ab6b6b4ae3999ef28a25bbb229e11eca975ab1ec1dd9b2a195783057b-2026-04-06-22-53-26-2d03a7b1ad5d42afbe941da8a399d4b1(1).zip`
- 대화 수: `41`
- `conversations.json` 크기: 약 `28.5MB`

### conversations.json 구조

```json
[
  {
    "title": "프로젝트 설계 논의",
    "create_time": 1711234567.123,
    "update_time": 1711234999.456,
    "mapping": {
      "msg-id-1": {
        "id": "msg-id-1",
        "message": {
          "id": "msg-id-1",
          "author": { "role": "system" },
          "content": { "content_type": "text", "parts": ["You are ChatGPT..."] },
          "create_time": 1711234567.123,
          "metadata": { "model_slug": "gpt-4" }
        },
        "parent": null,
        "children": ["msg-id-2"]
      },
      "msg-id-2": {
        "id": "msg-id-2",
        "message": {
          "author": { "role": "user" },
          "content": { "content_type": "text", "parts": ["설계 도와줘"] },
          "create_time": 1711234570.0
        },
        "parent": "msg-id-1",
        "children": ["msg-id-3"]
      }
    },
    "conversation_id": "conv-uuid-123",
    "default_model_slug": "gpt-4",
    "current_node": "msg-id-last"
  }
]
```

실제 top-level key:

- `conversation_id`, `id`, `title`, `create_time`, `update_time`
- `mapping`, `current_node`, `default_model_slug`
- `async_status`, `atlas_mode_enabled`, `context_scopes`
- `conversation_origin`, `conversation_template_id`
- `disabled_tool_ids`, `gizmo_type`, `is_archived`, `is_do_not_remember`, `is_starred`
- `memory_scope`, `moderation_results`, `voice`

### 실제 관찰 결과

#### 1. mapping 트리

- `mapping`은 `HashMap<String, MappingNode>` 형태
- 노드 구조는 `id`, `message`, `parent`, `children`
- `41`개 대화 중 `5`개에서 `children.len() > 1` 분기 존재
- `current_node`에서 `parent`를 따라가면 사용자가 마지막으로 본 경로를 선형화 가능
- `message == null`인 노드가 있을 수 있으므로 skip 필요

#### 2. 역할 분포

- `assistant`: `2378`
- `system`: `992`
- `user`: `767`
- `tool`: `742`

`tool` 메시지가 실제로 많이 존재하므로 skip하지 말고 `Role::System` 턴으로 보존하는 편이 안전합니다.

#### 3. 컨텐츠 타입 분포

- `text`: `3127`
- `thoughts`: `727`
- `code`: `442`
- `reasoning_recap`: `373`
- `multimodal_text`: `79`
- `execution_output`: `72`
- `user_editable_context`: `46`
- `tether_browsing_display`: `13`

예상과 달리 `thoughts`, `reasoning_recap`, `execution_output`, `user_editable_context`가 실제로 등장합니다.

#### 4. content 구조 차이

- `text` / `code`: 주로 `parts: [string, ...]`
- `multimodal_text`: `parts` 안에 이미지 asset object + 문자열이 함께 존재
- `execution_output`: `parts` 대신 `text` 필드 사용
- `reasoning_recap`: `content` 필드 사용
- `thoughts`: `thoughts: []` 배열 + `source_analysis_msg_id`
- `user_editable_context`: `parts` 없음, `user_profile` / `user_instructions` 사용
- `tether_browsing_display`: `result`, `summary`, `assets` 사용

즉, `content.parts[]`만 보면 일부 메시지 본문이 유실되므로 `content_type`별 추출 분기가 필요합니다.

#### 5. 모델 정보

실제 관찰된 모델 slug:

- `gpt-5-4-thinking`
- `gpt-5-2-thinking`
- `gpt-5-3`
- `gpt-5-2`
- `gpt-5-4-pro`

대화 단위 `default_model_slug`와 메시지 단위 `metadata.model_slug` / `metadata.resolved_model_slug`가 공존합니다. 세션 대표 모델은 `default_model_slug` 우선, 없으면 마지막 assistant/tool 메시지의 `model_slug` fallback이 적절합니다.

#### 6. 타임스탬프

- 대화 `create_time` / `update_time`: Unix epoch float
- 메시지 `create_time` / `update_time`: Unix epoch float
- 샘플 데이터에서 대화 `create_time == null`은 없었음 (`0/41`)

#### 7. 첨부/브라우징/툴 메시지

- ZIP 안에 실제 첨부 이미지/텍스트 파일이 같이 포함됨
- `multimodal_text`는 asset pointer object를 포함
- `tool` 메시지는 browsing / execution 결과를 담고 있으며, 일부는 빈 `parts: [""]` 이고 의미 있는 정보가 `metadata` 또는 다른 `content_type`에 존재
- `tether_browsing_display`는 검색/브라우징 상태 문자열을 `result`에 보관

### 파서 설계 반영

1. 선형화는 `current_node -> parent -> ... -> root` 체인 역추적으로 구현
2. `system`과 `user_editable_context`는 시각적으로 숨겨진 프롬프트 성격이 강하므로 기본 skip
3. `tool`은 보존하되 `Role::System`으로 매핑
4. `thoughts` / `reasoning_recap`은 assistant turn의 `thinking` 또는 본문 요약으로 반영
5. `execution_output`, `tether_browsing_display`, `multimodal_text`는 `content_type`별 텍스트 추출 로직 필요
6. `parts` 내 object는 `[attachment]` 같은 placeholder로 보존
7. 세션 제목은 `title`, 세션 ID는 `conversation_id`, 대표 모델은 `default_model_slug`

### 핵심 차이점 (vs claude.ai)

| 항목 | claude.ai | ChatGPT |
|---|---|---|
| 메시지 구조 | 선형 배열 `chat_messages[]` | 트리 `mapping{}` (parent/children) |
| 타임스탬프 | ISO 8601 문자열 | Unix epoch float |
| 역할 | `sender: "human"/"assistant"` | `author.role: "user"/"assistant"/"system"/"tool"` |
| 컨텐츠 | `content: [ContentBlock]` | `content_type`별 이형 구조 (`parts` / `text` / `content` / `thoughts`) |
| 모델 정보 | 없음 | `default_model_slug` + `metadata.model_slug` |
| ID | `uuid` (UUID) | `conversation_id` (UUID) |

## Constraints

- 실제 export 기준으로 `content_type` 변형이 많아 단순 `parts[]` 파싱으로는 불충분
- message tree → `current_node` parent chain 선형화 필요
- 트리에 분기(regeneration)가 있을 수 있음 → 마지막 활성 경로만 추출
- 대량의 `tool` / `thoughts` 메시지를 어떻게 보존할지 정책 필요

## Non-goals

- ChatGPT API 연동 (export 파일만)
- GPT 플러그인/Code Interpreter/DALL-E 결과물 별도 처리 (텍스트만 추출)
- Canvas/Artifacts 구조 파싱
- shared_conversations.json 파싱
