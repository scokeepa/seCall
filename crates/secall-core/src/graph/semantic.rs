use anyhow::Result;
use serde::Deserialize;

use crate::ingest::markdown::SessionFrontmatter;
use crate::store::Database;

use super::extract::{extract_semantic_edges, GraphEdge};

// ─── Haiku API 응답 구조 ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct HaikuResponse {
    content: Vec<HaikuContent>,
}

#[derive(Debug, Deserialize)]
struct HaikuContent {
    text: String,
}

#[derive(Debug, Deserialize)]
struct SemanticOutput {
    edges: Vec<SemanticEdgeItem>,
}

#[derive(Debug, Deserialize)]
struct SemanticEdgeItem {
    relation: String,
    target_type: String,
    target_label: String,
}

// ─── 정적 프롬프트 ──────────────────────────────────────────────────────────

const SYSTEM_PROMPT: &str = r#"Extract semantic relationships from this agent session log. Return JSON only.

Output schema:
{"edges": [{"relation": "fixes_bug|modifies_file|introduces_tech|discusses_topic", "target_type": "issue|file|tech|topic", "target_label": "<value>"}]}

Rules:
- fixes_bug: issue references like #N, "closes #N", "fixes #N"
- modifies_file: file paths that were edited or written
- introduces_tech: new technology, library, or tool introduced
- discusses_topic: main topic or concept discussed
- Return empty edges array if nothing is found
- target_label for issues: just the number (e.g. "15")
- target_label for files: relative path (e.g. "src/main.rs")
- Do not include trivial relationships"#;

const HAIKU_MODEL: &str = "claude-haiku-4-5-20251001";
const HAIKU_MAX_TOKENS: u32 = 512;
const API_URL: &str = "https://api.anthropic.com/v1/messages";
const BODY_LIMIT: usize = 2000;

// ─── Haiku API 호출 ─────────────────────────────────────────────────────────

/// Haiku API로 시맨틱 엣지 추출.
/// `ANTHROPIC_API_KEY` 환경변수가 필요하며, 없으면 Err 반환.
pub async fn extract_with_haiku(fm: &SessionFrontmatter, body: &str) -> Result<Vec<GraphEdge>> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| anyhow::anyhow!("ANTHROPIC_API_KEY not set"))?;

    let truncated_body = if body.len() > BODY_LIMIT {
        &body[..body
            .char_indices()
            .take_while(|(i, _)| *i < BODY_LIMIT)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(BODY_LIMIT)]
    } else {
        body
    };

    let user_content = format!(
        "---\nsession_id: {}\nagent: {}\nproject: {}\ndate: {}\nsummary: {}\n---\n\n{}",
        fm.session_id,
        fm.agent,
        fm.project.as_deref().unwrap_or(""),
        fm.date,
        fm.summary.as_deref().unwrap_or(""),
        truncated_body,
    );

    let request_body = serde_json::json!({
        "model": HAIKU_MODEL,
        "max_tokens": HAIKU_MAX_TOKENS,
        "system": SYSTEM_PROMPT,
        "messages": [{"role": "user", "content": user_content}]
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(API_URL)
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&request_body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Haiku API error {}: {}", status, text);
    }

    let haiku_resp: HaikuResponse = resp.json().await?;
    let text = haiku_resp
        .content
        .first()
        .map(|c| c.text.as_str())
        .unwrap_or("{}");

    parse_haiku_edges(text, &fm.session_id)
}

/// Haiku JSON 응답 → GraphEdge 변환
fn parse_haiku_edges(json_text: &str, session_id: &str) -> Result<Vec<GraphEdge>> {
    let output: SemanticOutput = serde_json::from_str(json_text)
        .map_err(|e| anyhow::anyhow!("failed to parse Haiku JSON: {}", e))?;

    let session_node_id = format!("session:{}", session_id);

    let weight_for = |relation: &str| -> f64 {
        match relation {
            "fixes_bug" => 0.8,
            "modifies_file" => 0.9,
            "introduces_tech" => 0.6,
            "discusses_topic" => 0.5,
            _ => 0.5,
        }
    };

    let edges = output
        .edges
        .into_iter()
        .filter(|item| !item.target_label.is_empty())
        .map(|item| {
            let target_id = format!("{}:{}", item.target_type, item.target_label);
            GraphEdge {
                source: session_node_id.clone(),
                target: target_id,
                relation: item.relation.clone(),
                confidence: "LLM".to_string(),
                weight: weight_for(&item.relation),
            }
        })
        .collect();

    Ok(edges)
}

// ─── 통합 저장 ──────────────────────────────────────────────────────────────

/// 규칙 기반 + (옵션) Haiku API로 시맨틱 엣지 추출 후 DB 저장.
///
/// - 항상 규칙 기반 실행
/// - `ANTHROPIC_API_KEY` 존재 시 Haiku 호출 → 실패 시 규칙 결과만 저장
/// - 노드 자동 생성 (issue:N, file:path, tech:X, topic:Y)
/// - 중복은 DB UNIQUE 제약으로 자동 방어
pub async fn extract_and_store(
    db: &Database,
    fm: &SessionFrontmatter,
    body: &str,
) -> Result<usize> {
    // 1. 규칙 기반 — 항상 실행
    let mut all_edges = extract_semantic_edges(fm, body);

    // 2. Haiku API — API key 있을 때만 시도
    if std::env::var("ANTHROPIC_API_KEY").is_ok() {
        match extract_with_haiku(fm, body).await {
            Ok(haiku_edges) => {
                tracing::debug!(
                    session = &fm.session_id[..fm.session_id.len().min(8)],
                    haiku_edges = haiku_edges.len(),
                    "Haiku edges extracted"
                );
                all_edges.extend(haiku_edges);
            }
            Err(e) => {
                tracing::warn!(
                    session = &fm.session_id[..fm.session_id.len().min(8)],
                    "Haiku API failed, using rules only: {}",
                    e
                );
            }
        }
    }

    // 3. 중복 제거: (source, target, relation) 기준, 먼저 추출된 엣지 우선
    {
        let mut seen = std::collections::HashSet::new();
        all_edges.retain(|e| seen.insert((e.source.clone(), e.target.clone(), e.relation.clone())));
    }

    // 4. DB 저장
    let mut stored = 0usize;
    for edge in &all_edges {
        // 타겟 노드 자동 생성
        let (target_type, target_label) = if let Some(rest) = edge.target.strip_prefix("issue:") {
            ("issue", rest)
        } else if let Some(rest) = edge.target.strip_prefix("file:") {
            ("file", rest)
        } else if let Some(rest) = edge.target.strip_prefix("tech:") {
            ("tech", rest)
        } else if let Some(rest) = edge.target.strip_prefix("topic:") {
            ("topic", rest)
        } else {
            ("unknown", edge.target.as_str())
        };

        // 소스 세션 노드가 없으면 최소 생성
        let session_node_id = format!("session:{}", fm.session_id);
        let session_label = fm.session_id[..fm.session_id.len().min(8)].to_string();
        db.upsert_graph_node(&session_node_id, "session", &session_label, None)?;

        db.upsert_graph_node(&edge.target, target_type, target_label, None)?;
        let rows = db.upsert_graph_edge(
            &edge.source,
            &edge.target,
            &edge.relation,
            &edge.confidence,
            edge.weight,
        )?;
        stored += rows;
    }

    Ok(stored)
}

// ─── 테스트 ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_fm(id: &str, tools: Option<Vec<&str>>, summary: Option<&str>) -> SessionFrontmatter {
        SessionFrontmatter {
            session_id: id.to_string(),
            agent: "claude-code".to_string(),
            model: None,
            project: Some("test-project".to_string()),
            cwd: None,
            date: "2026-04-13".to_string(),
            start_time: "2026-04-13T00:00:00Z".to_string(),
            end_time: None,
            turns: Some(5),
            tokens_in: None,
            tokens_out: None,
            tools_used: tools.map(|t| t.iter().map(|s| s.to_string()).collect()),
            host: None,
            status: None,
            summary: summary.map(|s| s.to_string()),
            session_type: None,
        }
    }

    #[tokio::test]
    async fn test_extract_and_store_rules_only() {
        // ANTHROPIC_API_KEY 없이 → 규칙 기반만 동작
        std::env::remove_var("ANTHROPIC_API_KEY");

        let db = Database::open_memory().unwrap();
        let fm = make_fm("sess001", Some(vec!["Edit"]), Some("closes #42"));
        let body = "> [!tool]- Edit `src/main.rs`\n\nsome content";

        let stored = extract_and_store(&db, &fm, body).await.unwrap();

        // fixes_bug(#42) + modifies_file(src/main.rs) = 2
        assert!(stored >= 2, "expected at least 2 edges, got {}", stored);

        // 노드가 생성되었는지 확인
        let neighbors = db.get_neighbors("session:sess001").unwrap();
        assert!(
            neighbors
                .iter()
                .any(|(id, rel, _)| id == "issue:42" && rel == "fixes_bug"),
            "fixes_bug edge to issue:42 expected"
        );
        assert!(
            neighbors
                .iter()
                .any(|(id, rel, _)| id == "file:src/main.rs" && rel == "modifies_file"),
            "modifies_file edge to file:src/main.rs expected"
        );
    }

    #[test]
    fn test_haiku_response_parsing() {
        let json = r#"{"edges": [
            {"relation": "fixes_bug", "target_type": "issue", "target_label": "15"},
            {"relation": "introduces_tech", "target_type": "tech", "target_label": "ONNX Runtime"}
        ]}"#;

        let edges = parse_haiku_edges(json, "test-session").unwrap();
        assert_eq!(edges.len(), 2);

        assert_eq!(edges[0].relation, "fixes_bug");
        assert_eq!(edges[0].target, "issue:15");
        assert_eq!(edges[0].confidence, "LLM");
        assert!((edges[0].weight - 0.8).abs() < f64::EPSILON);

        assert_eq!(edges[1].relation, "introduces_tech");
        assert_eq!(edges[1].target, "tech:ONNX Runtime");
        assert_eq!(edges[1].confidence, "LLM");
        assert!((edges[1].weight - 0.6).abs() < f64::EPSILON);
    }

    #[test]
    fn test_haiku_invalid_json_fallback() {
        let bad_json = "not a json at all";
        let result = parse_haiku_edges(bad_json, "test-session");
        assert!(result.is_err(), "invalid JSON should return Err");
    }

    /// Haiku API가 실패해도 extract_and_store는 규칙 기반 결과를 저장해야 한다.
    /// (ANTHROPIC_API_KEY가 없으면 Haiku를 시도하지 않으므로 규칙 기반만 실행)
    #[tokio::test]
    async fn test_extract_and_store_fallback_without_api_key() {
        std::env::remove_var("ANTHROPIC_API_KEY");

        let db = Database::open_memory().unwrap();
        let fm = make_fm("fallback01", Some(vec!["Edit"]), Some("closes #7"));
        let body = "> [!tool]- Edit `src/lib.rs`\n\nsome code";

        let stored = extract_and_store(&db, &fm, body).await.unwrap();

        // API key 없이도 규칙 기반 결과가 저장되어야 함
        assert!(
            stored >= 2,
            "expected at least 2 rule-based edges, got {}",
            stored
        );

        // DB에 실제 엣지 존재 확인
        let neighbors = db.get_neighbors("session:fallback01").unwrap();
        assert!(
            neighbors
                .iter()
                .any(|(id, rel, _)| id == "issue:7" && rel == "fixes_bug"),
            "fixes_bug edge expected even without Haiku"
        );
        assert!(
            neighbors
                .iter()
                .any(|(id, rel, _)| id == "file:src/lib.rs" && rel == "modifies_file"),
            "modifies_file edge expected even without Haiku"
        );
    }

    /// 동일 세션에 대해 extract_and_store를 두 번 호출하면 두 번째는 0을 반환해야 한다.
    #[tokio::test]
    async fn test_extract_and_store_double_call_returns_zero() {
        std::env::remove_var("ANTHROPIC_API_KEY");

        let db = Database::open_memory().unwrap();
        let fm = make_fm("double01", Some(vec!["Edit"]), Some("closes #99"));
        let body = "> [!tool]- Edit `src/app.rs`\n\nsome content";

        let first = extract_and_store(&db, &fm, body).await.unwrap();
        assert!(
            first >= 2,
            "first call should store at least 2 edges, got {}",
            first
        );

        let second = extract_and_store(&db, &fm, body).await.unwrap();
        assert_eq!(
            second, 0,
            "second call should return 0 (all edges already exist)"
        );
    }

    /// 규칙 기반과 Haiku가 같은 엣지를 생성해도 stored 카운트가 정확해야 한다.
    #[test]
    fn test_dedup_edges_before_store() {
        let edges = vec![
            GraphEdge {
                source: "session:s1".to_string(),
                target: "issue:42".to_string(),
                relation: "fixes_bug".to_string(),
                confidence: "INFERRED".to_string(),
                weight: 0.7,
            },
            GraphEdge {
                source: "session:s1".to_string(),
                target: "issue:42".to_string(),
                relation: "fixes_bug".to_string(),
                confidence: "LLM".to_string(),
                weight: 0.8,
            },
        ];

        // 수동으로 dedup 로직 검증
        let mut deduped = edges;
        {
            let mut seen = std::collections::HashSet::new();
            deduped
                .retain(|e| seen.insert((e.source.clone(), e.target.clone(), e.relation.clone())));
        }
        assert_eq!(deduped.len(), 1, "duplicate edges should be removed");
        // 먼저 추출된 (규칙 기반) 엣지가 우선
        assert_eq!(deduped[0].confidence, "INFERRED");
    }
}
