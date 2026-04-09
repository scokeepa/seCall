# Changelog

## v0.2.3 (2026-04-09)

### Added
- ChatGPT export 파서 (`ChatGptParser`) — `conversations.json` ZIP/JSON 파싱
- `mapping` HashMap → `current_node` 부모 체인 추적으로 대화 선형화 (재생성 분기 자동 처리)
- 멀티 content type 지원: text, code, multimodal_text, execution_output, reasoning_recap, thoughts, tether_browsing_display, user_editable_context
- `AgentKind::ChatGpt` variant
- `detect.rs`에 ChatGPT export 자동 탐지 로직 추가

## v0.2.2 (2026-04-08)

### Added
- `config.toml`에 `[output] timezone` 설정 추가 — IANA 타임존(예: `Asia/Seoul`)으로 vault 마크다운 타임스탬프 렌더링. 기본값 UTC.

### Changed
- vault 디렉토리 경로(`raw/sessions/YYYY-MM-DD/`)가 설정된 타임존 기준 날짜로 생성
- frontmatter `start_time`/`end_time`에 동적 UTC 오프셋 적용 (예: `+09:00`)

## v0.2.1 (2026-04-08)

### Added
- `secall ingest --force` — 이미 인덱싱된 세션도 강제 재수집. vault MD 재생성 + DB 재삽입. claude.ai 재export나 렌더링 변경 적용 시 사용.

### Fixed
- Dataview inline field 오염 방지 — vault 마크다운 body의 `::` 패턴에 zero-width space 삽입하여 Dataview가 인라인 필드로 해석하지 않도록 처리. fenced code block / inline code 내부는 보존.

## v0.2.0 (2026-04-07)

### Added
- claude.ai 공식 export JSON 파서 (`ClaudeAiParser`)
- ZIP 자동 해제 지원 (`secall ingest <export.zip>`)
- `AgentKind::ClaudeAi` variant
- `SessionParser::parse_all()` — 1:N 파싱 지원

### Changed
- `AgentKind` enum에 `ClaudeAi` variant 추가
- `detect.rs`에 claude.ai export 자동 탐지 로직 추가

## v0.1.0 (2026-04-06)

### Added
- 초기 릴리스
- Claude Code / Codex CLI / Gemini CLI 파서
- BM25 + 벡터 하이브리드 검색 (RRF k=60)
- MCP 서버 (stdio + HTTP)
- Obsidian 호환 vault 구조
- Git 기반 멀티 기기 동기화 (`secall sync`)
- ANN 인덱스 (usearch HNSW)
- CI/CD GitHub Actions
