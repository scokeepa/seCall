---
type: task
plan: p25-phase-2-graph
task_number: 2
status: draft
title: Obsidian 데일리 노트 뷰 + 노트 생성
depends_on: [1]
parallel_group: views
updated_at: 2026-04-14
---

# Task 02 — Obsidian 데일리 노트 뷰 + 노트 생성

## Changed files

1. **`obsidian-secall/src/daily-view.ts`** (신규)
   - `DailyView` (extends `ItemView`) — 데일리 노트 생성 뷰

2. **`obsidian-secall/src/api.ts:1-36`** (수정)
   - `daily(date?: string)` 메서드 추가

3. **`obsidian-secall/src/settings.ts:4-9`** (수정)
   - `SeCallSettings`에 `dailyNotesFolder` 필드 추가
   - `SeCallSettingTab.display()`에 폴더 설정 UI 추가

4. **`obsidian-secall/src/main.ts:1-73`** (수정)
   - `DailyView` import + registerView 추가
   - `secall-daily-note` 커맨드 추가 (오늘 날짜로 데일리 뷰 열기)

5. **`obsidian-secall/styles.css`** (수정)
   - 데일리 뷰 관련 CSS 추가

## Change description

### Step 1: `api.ts` — `daily()` 메서드

```typescript
async daily(date?: string) {
  const resp = await requestUrl({
    url: `${this.baseUrl}/api/daily`,
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ date }),
  });
  return resp.json;
}
```

### Step 2: `settings.ts` — 데일리 노트 폴더 설정

`SeCallSettings` 인터페이스에 추가:
```typescript
export interface SeCallSettings {
  serverUrl: string;
  dailyNotesFolder: string;  // 기본 "seCall/daily"
}
```

`DEFAULT_SETTINGS`에 `dailyNotesFolder: "seCall/daily"` 추가.
`SeCallSettingTab.display()`에 텍스트 입력 필드 추가.

### Step 3: `daily-view.ts` — DailyView 구현

**UI 구조:**
```
┌─────────────────────────────────┐
│ 📅 날짜 선택 [< ] [2026-04-14] [>]│
├─────────────────────────────────┤
│ 총 15세션 (필터 후 10) │ 토픽: rust, graph │
├─────────────────────────────────┤
│ ## seCall                       │
│   - (5턴) P25 REST API 구현      │
│   - (3턴) 스모크 테스트             │
│ ## tunaFlow                     │
│   - (8턴) 대시보드 레이아웃         │
├─────────────────────────────────┤
│ [📝 노트 생성]                    │
└─────────────────────────────────┘
```

**핵심 로직:**

1. `onOpen()`: 오늘 날짜로 초기화, `fetchDaily()` 호출
2. `fetchDaily(date)`: `this.plugin.api.daily(date)` 호출 → UI 렌더링
3. 날짜 이동: `<` / `>` 버튼으로 ±1일 이동
4. 프로젝트별 세션 그룹 렌더링: `data.projects` 객체 순회
5. 세션 클릭 → `openSession(session_id)` (기존 SearchView 패턴 재사용)
6. `createNote()`:
   - 마크다운 생성 (프로젝트별 헤더 + 세션 요약 목록 + 토픽)
   - `this.app.vault.create(path, content)` 또는 기존 파일이면 사용자에게 덮어쓰기 확인
   - 경로: `{dailyNotesFolder}/{YYYY-MM-DD}.md`
   - 생성 후 해당 파일 열기

**노트 마크다운 템플릿** (CLI `log.rs:189-208`의 `generate_template`과 동일 형태):
```markdown
# 2026-04-14 작업 일지

## seCall
- (5턴, 도구:[Edit, Read]) P25 REST API 구현
- (3턴, 도구:[Bash]) 스모크 테스트

## tunaFlow
- (8턴, 도구:[Edit]) 대시보드 레이아웃

**주요 토픽**: rust, graph, api

*총 10개 세션*
```

### Step 4: `main.ts` — 뷰 등록 + 커맨드

```typescript
import { DailyView, DAILY_VIEW_TYPE } from "./daily-view";

// onload() 내:
this.registerView(DAILY_VIEW_TYPE, (leaf) => new DailyView(leaf, this));

this.addCommand({
  id: "secall-daily-note",
  name: "Daily Note",
  callback: () => this.openDailyView(),
});
```

`openDailyView()` 메서드: 기존 `openSearchView()` 패턴과 동일하게 구현.

### Step 5: `styles.css` — 데일리 뷰 스타일

```css
.secall-daily-header { padding: 12px; border-bottom: 1px solid var(--background-modifier-border); }
.secall-daily-nav { display: flex; align-items: center; gap: 8px; }
.secall-daily-date { font-weight: 600; min-width: 120px; text-align: center; }
.secall-daily-stats { font-size: 0.85em; color: var(--text-muted); margin-top: 4px; }
.secall-daily-projects { padding: 12px; }
.secall-daily-project h4 { margin: 8px 0 4px 0; }
.secall-daily-session { padding: 4px 0; cursor: pointer; }
.secall-daily-session:hover { color: var(--text-accent); }
.secall-daily-actions { padding: 12px; border-top: 1px solid var(--background-modifier-border); }
```

## Dependencies

- **Task 01** — REST `/api/daily` 엔드포인트가 존재해야 함
- Obsidian API: `ItemView`, `TFile`, `vault.create()`, `vault.getAbstractFileByPath()`

## Verification

```bash
# 1. TypeScript 컴파일 확인
cd obsidian-secall && npx tsc --noEmit 2>&1 | tail -5
# 기대: no errors

# 2. esbuild 번들 생성
cd obsidian-secall && node esbuild.config.mjs production 2>&1
# 기대: main.js 생성 성공

# 3. API daily 메서드 존재 확인
grep -n "async daily" obsidian-secall/src/api.ts
# 기대: daily 메서드 정의 출력

# 4. DailyView export 확인
grep -n "DAILY_VIEW_TYPE" obsidian-secall/src/daily-view.ts
# 기대: export const DAILY_VIEW_TYPE 출력

# 5. 커맨드 등록 확인
grep -n "secall-daily-note" obsidian-secall/src/main.ts
# 기대: id: "secall-daily-note" 출력
```

## Risks

- `vault.create()` 호출 시 이미 파일이 존재하면 에러 → `getAbstractFileByPath()`로 먼저 확인하고, 존재하면 `vault.modify()`로 덮어쓰기 (사용자에게 Notice로 알림)
- 날짜 이동 시 매번 API 호출 → 짧은 debounce 또는 로딩 인디케이터 표시
- `dailyNotesFolder` 경로에 폴더가 없으면 → `vault.createFolder()` 선행 호출

## Scope boundary — 수정 금지

- `crates/` — Rust 코드 수정 금지 (Task 01 영역)
- `obsidian-secall/src/search-view.ts` — 기존 검색 뷰 수정 금지
- `obsidian-secall/src/session-view.ts` — 기존 세션 뷰 수정 금지
- `obsidian-secall/src/graph-view.ts` — Task 03 영역
