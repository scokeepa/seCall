---
type: plan
status: draft
version: "1.0"
updated_at: 2026-04-06
slug: gemini
title: "Gemini 파서 핫픽스"
---

# Gemini 파서 핫픽스

## Description

실제 Gemini CLI JSON 형식에 맞게 파서를 재작성한다.

현재 파서(`gemini.rs`)는 Google AI Studio API 형식(`id`, `createTime`, `messages[].role`, `messages[].parts[].text`)을 기대하지만, 실제 Gemini CLI(v3.1+)는 완전히 다른 구조를 사용한다.

### 실제 Gemini CLI JSON 구조 (79개 세션 분석 결과)

**세션 최상위**:
```json
{
  "sessionId": "uuid",
  "projectHash": "sha256-hash",
  "startTime": "ISO8601",
  "lastUpdated": "ISO8601",
  "kind": "main",
  "messages": [...]
}
```
- 키 패턴이 79개 파일 전부 동일: `{kind, lastUpdated, messages, projectHash, sessionId, startTime}`

**메시지 타입 (3종)**:

| type | 용도 | 고유 필드 |
|---|---|---|
| `user` | 사용자 입력 | `id`, `timestamp`, `content` |
| `gemini` | 모델 응답 | `id`, `timestamp`, `content`, `thoughts`, `tokens`, `model`, `toolCalls` |
| `info` | 시스템 알림 | `id`, `timestamp`, `content` (항상 string) |

**content 타입**: `[{text: "..."}]` (array) 또는 `"string"` (직접 문자열) — 두 형태 모두 존재

**toolCalls 구조** (gemini 메시지 내부):
```json
{
  "id": "read_file_123_0",
  "name": "read_file",
  "args": {"file_path": "/path/to/file"},
  "result": [{
    "functionResponse": {
      "id": "read_file_123_0",
      "name": "read_file",
      "response": {"output": "file content..."}
    }
  }],
  "status": "success|error",
  "timestamp": "ISO8601",
  "displayName": "ReadFile"
}
```
- **핵심 차이**: call과 result가 **같은 객체 안에** 존재. 별도 `role: "function"` 메시지가 없음.

**thoughts 구조**:
```json
[{"subject": "...", "description": "...", "timestamp": "ISO8601"}]
```

**tokens 구조**:
```json
{"input": N, "output": N, "cached": N, "thoughts": N, "tool": N, "total": N}
```

### 현재 파서와의 차이

| 항목 | 파서 기대 | 실제 |
|---|---|---|
| session ID | `id` | `sessionId` |
| timestamps | `createTime`/`updateTime` | `startTime`/`lastUpdated` |
| message role | `role: "user"/"model"/"function"` | `type: "user"/"gemini"/"info"` |
| content | `parts: [{text}]` | `content: [{text}]` 또는 `content: "string"` |
| tool calls | 별도 `role: "function"` 메시지 + pending queue | 같은 메시지 내 `toolCalls[]` (결과 포함) |
| thinking | 없음 | `thoughts[]` |
| tokens | 없음 | `tokens` 객체 |
| model | 없음 | `model` 문자열 |
| per-message timestamp | 없음 | `timestamp` 필드 |

## Expected Outcome

- `~/.gemini/tmp/` 하위 79개 세션 파일 전부 파싱 성공
- `sessionId` → Session.id
- `startTime`/`lastUpdated` → start_time/end_time
- `type: "user"` → User 턴, `type: "gemini"` → Assistant 턴
- `type: "info"` → skip
- `toolCalls[]` → Turn.actions (call + result가 같은 객체)
- `thoughts` → Turn.thinking
- `tokens.input`/`tokens.output` → Turn.tokens (TokenUsage)
- `model` → Session.model (첫 번째 gemini 메시지에서 추출)
- per-message `timestamp` → Turn.timestamp

## Subtasks

1. **gemini.rs 파서 재작성** — serde 모델을 실제 구조로 교체, parse 함수 재작성, 테스트를 실제 형식으로 업데이트
   - parallel_group: —
   - depends_on: —

## Constraints

- `Session`, `Turn`, `Action`, `TokenUsage` 구조체는 변경하지 않음
- 다른 파서(claude.rs, codex.rs)는 건드리지 않음
- `projectHash`는 해시값이라 프로젝트명으로 부적합 → 기존 `extract_project_id()` (경로 기반) 유지

## Non-goals

- `kind` 필드 분기 ("main" vs 기타)
- `projectHash` → 실제 프로젝트명 역매핑
- `thoughts.description` 요약/정리 (raw 텍스트 그대로 저장)
- `tokens.cached`, `tokens.thoughts`, `tokens.tool` 세부 분류 (input/output만 사용)
- `toolCalls[].displayName`, `toolCalls[].description`, `toolCalls[].renderOutputAsMarkdown` 등 UI 메타 필드

## Risks

- **Gemini CLI 버전 차이**: 분석은 v3.1-pro-preview 기준. 이전 버전은 다른 형식일 수 있음. `serde(default)` + skip으로 방어.
- **content 타입 혼재**: array와 string 두 형태. `#[serde(untagged)]` 또는 수동 처리 필요.
- **toolCalls.result 배열**: result가 배열 형태 `[{functionResponse: {...}}]`. 단일 항목만 있을 것으로 예상하나, 복수 결과 방어 필요.
