---
type: task
plan: p25-phase-2-graph
task_number: 3
status: draft
title: Obsidian Graph 탐색 뷰
depends_on: [1]
parallel_group: views
updated_at: 2026-04-14
---

# Task 03 — Obsidian Graph 탐색 뷰

## Changed files

1. **`obsidian-secall/src/graph-view.ts`** (신규)
   - `GraphView` (extends `ItemView`) — 인터랙티브 그래프 탐색 트리 뷰

2. **`obsidian-secall/src/api.ts`** (수정)
   - `graph(nodeId: string, depth?: number, relation?: string)` 메서드 추가

3. **`obsidian-secall/src/main.ts`** (수정)
   - `GraphView` import + registerView 추가
   - `secall-graph` 커맨드 추가
   - SearchView 검색 결과에서 graph 탐색 진입점 연결

4. **`obsidian-secall/src/search-view.ts`** (수정 — 최소)
   - 검색 결과 아이템에 "Graph" 버튼 추가 (세션 노드로 graph-view 열기)

5. **`obsidian-secall/styles.css`** (수정)
   - Graph 뷰 관련 CSS 추가

## Change description

### Step 1: `api.ts` — `graph()` 메서드

```typescript
async graph(nodeId: string, depth = 1, relation?: string) {
  const resp = await requestUrl({
    url: `${this.baseUrl}/api/graph`,
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ node_id: nodeId, depth, relation }),
  });
  return resp.json;
}
```

### Step 2: `graph-view.ts` — GraphView 구현

**UI 구조:**
```
┌────────────────────────────────────┐
│ 🔍 [node_id 입력]  [depth: 1 ▾]    │
├────────────────────────────────────┤
│ 📌 project:seCall (project)        │
│ ├─ 🔗 session:abc... [same_project]│
│ │   └─ 클릭하면 세션 열기            │
│ ├─ 🔗 topic:rust [discusses_topic] │
│ │   └─ 클릭하면 이 노드로 재탐색     │
│ ├─ 🔗 tool:Edit [uses_tool]        │
│ └─ 🔗 agent:claude-code [by_agent] │
├────────────────────────────────────┤
│ 총 12개 연결                        │
└────────────────────────────────────┘
```

**핵심 로직:**

1. `onOpen()`: 빈 상태 또는 전달된 `nodeId`로 초기화
2. `setState({ nodeId })`: 외부에서 노드 ID 전달받아 탐색 시작
3. `explore(nodeId)`: `this.plugin.api.graph(nodeId, depth)` 호출 → 트리 렌더링
4. **결과 노드 렌더링**: 각 노드를 타입별 아이콘으로 구분
   - `session:*` → 📄 (클릭 → SessionView로 열기)
   - `project:*` → 📁 (클릭 → 해당 노드로 재탐색)
   - `topic:*` → 🏷️ (클릭 → 재탐색)
   - `tool:*` → 🔧 (클릭 → 재탐색)
   - `agent:*` → 🤖 (클릭 → 재탐색)
   - `file:*` → 📝 (클릭 → 재탐색)
5. **Breadcrumb 히스토리**: 탐색한 노드 경로를 상단에 표시, 클릭으로 뒤로가기
6. **relation 필터**: 드롭다운으로 특정 relation만 표시 (전체 / same_project / discusses_topic / uses_tool / by_agent / ...)

**노드 타입별 아이콘 매핑:**
```typescript
const NODE_ICONS: Record<string, string> = {
  session: "file-text",
  project: "folder",
  topic: "tag",
  tool: "wrench",
  agent: "bot",
  file: "file-code",
  issue: "alert-circle",
};
```

아이콘은 Obsidian의 내장 Lucide 아이콘 사용 (`setIcon(el, iconName)`).

### Step 3: `main.ts` — 뷰 등록 + 커맨드

```typescript
import { GraphView, GRAPH_VIEW_TYPE } from "./graph-view";

// onload() 내:
this.registerView(GRAPH_VIEW_TYPE, (leaf) => new GraphView(leaf, this));

this.addCommand({
  id: "secall-graph",
  name: "Graph Explorer",
  callback: () => this.openGraphView(),
});
```

`openGraphView(nodeId?: string)` 메서드 추가: 기존 `openSearchView()` 패턴 기반.
nodeId가 주어지면 setState로 전달.

### Step 4: `search-view.ts` — Graph 진입점

검색 결과 아이템 렌더링 (`doSearch()`) 내에서, 각 아이템에 작은 "Graph" 버튼 추가:

```typescript
const graphBtn = item.createEl("button", {
  text: "Graph",
  cls: "secall-graph-btn",
});
graphBtn.addEventListener("click", (e) => {
  e.stopPropagation();
  this.plugin.openGraphView(`session:${r.session_id}`);
});
```

### Step 5: `styles.css` — Graph 뷰 스타일

```css
.secall-graph-search { padding: 8px; display: flex; gap: 8px; }
.secall-graph-input { flex: 1; }
.secall-graph-breadcrumb { padding: 4px 12px; font-size: 0.85em; color: var(--text-muted); }
.secall-graph-breadcrumb span { cursor: pointer; }
.secall-graph-breadcrumb span:hover { color: var(--text-accent); }
.secall-graph-results { padding: 8px; }
.secall-graph-node { padding: 4px 8px; display: flex; align-items: center; gap: 6px; cursor: pointer; border-radius: 4px; }
.secall-graph-node:hover { background: var(--background-modifier-hover); }
.secall-graph-node-id { font-weight: 500; }
.secall-graph-node-relation { font-size: 0.8em; color: var(--text-muted); }
.secall-graph-node-label { font-size: 0.85em; color: var(--text-faint); }
.secall-graph-btn { font-size: 0.75em; padding: 2px 6px; cursor: pointer; }
.secall-graph-count { padding: 8px; font-size: 0.85em; color: var(--text-muted); border-top: 1px solid var(--background-modifier-border); }
```

## Dependencies

- **Task 01** — `/api/graph` 응답에 `node_type`, `label` 포함되어야 함
- Obsidian API: `ItemView`, `setIcon()`, `ViewStateResult`

## Verification

```bash
# 1. TypeScript 컴파일 확인
cd obsidian-secall && npx tsc --noEmit 2>&1 | tail -5
# 기대: no errors

# 2. esbuild 번들 생성
cd obsidian-secall && node esbuild.config.mjs production 2>&1
# 기대: main.js 생성 성공

# 3. GraphView export 확인
grep -n "GRAPH_VIEW_TYPE" obsidian-secall/src/graph-view.ts
# 기대: export const GRAPH_VIEW_TYPE 출력

# 4. 커맨드 등록 확인
grep -n "secall-graph" obsidian-secall/src/main.ts
# 기대: id: "secall-graph" 출력

# 5. search-view에 Graph 버튼 존재 확인
grep -n "secall-graph-btn" obsidian-secall/src/search-view.ts
# 기대: cls: "secall-graph-btn" 출력

# 6. API graph 메서드 존재 확인
grep -n "async graph" obsidian-secall/src/api.ts
# 기대: graph 메서드 정의 출력
```

## Risks

- Graph 결과 노드가 많을 때 DOM 과부하 → 최대 50개 노드까지만 렌더, "더 보기" 버튼으로 확장
- `session:*` 노드 클릭 시 session_id 파싱 필요 (`session:` prefix 제거)
- Breadcrumb 히스토리가 깊어지면 UI 넘침 → 최대 5단계, 초과 시 앞부분 `...` 처리
- `search-view.ts` 수정은 최소화: Graph 버튼 추가만 (기존 클릭 핸들러 미접촉)

## Scope boundary — 수정 금지

- `crates/` — Rust 코드 수정 금지 (Task 01 영역)
- `obsidian-secall/src/session-view.ts` — 기존 세션 뷰 수정 금지
- `obsidian-secall/src/daily-view.ts` — Task 02 영역
- `obsidian-secall/src/settings.ts` — Task 02 영역 (settings 확장은 Task 02에서)
