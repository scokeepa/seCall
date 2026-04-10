use std::collections::HashMap;

use crate::error::Result;

use super::db::Database;

#[derive(Debug, Default)]
pub struct GraphStats {
    pub node_count: usize,
    pub edge_count: usize,
    pub nodes_by_type: HashMap<String, usize>,
    pub edges_by_relation: HashMap<String, usize>,
}

impl Database {
    /// 노드 upsert (INSERT OR REPLACE)
    pub fn upsert_graph_node(
        &self,
        id: &str,
        node_type: &str,
        label: &str,
        meta: Option<&str>,
    ) -> Result<()> {
        self.conn().execute(
            "INSERT OR REPLACE INTO graph_nodes(id, type, label, meta) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![id, node_type, label, meta],
        )?;
        Ok(())
    }

    /// 엣지 upsert (INSERT OR IGNORE — 중복 무시)
    pub fn upsert_graph_edge(
        &self,
        source: &str,
        target: &str,
        relation: &str,
        confidence: &str,
        weight: f64,
    ) -> Result<()> {
        self.conn().execute(
            "INSERT OR IGNORE INTO graph_edges(source, target, relation, confidence, weight) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![source, target, relation, confidence, weight],
        )?;
        Ok(())
    }

    /// 노드의 이웃 조회 (양방향)
    /// 반환: Vec<(neighbor_id, relation, direction)>  direction: "out" | "in"
    pub fn get_neighbors(&self, node_id: &str) -> Result<Vec<(String, String, String)>> {
        let mut results = Vec::new();

        // 나가는 엣지 (source = node_id)
        let mut stmt = self
            .conn()
            .prepare("SELECT target, relation FROM graph_edges WHERE source = ?1")?;
        let out_rows = stmt.query_map([node_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in out_rows.filter_map(|r| r.ok()) {
            results.push((row.0, row.1, "out".to_string()));
        }

        // 들어오는 엣지 (target = node_id)
        let mut stmt = self
            .conn()
            .prepare("SELECT source, relation FROM graph_edges WHERE target = ?1")?;
        let in_rows = stmt.query_map([node_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in in_rows.filter_map(|r| r.ok()) {
            results.push((row.0, row.1, "in".to_string()));
        }

        Ok(results)
    }

    /// 그래프 통계
    pub fn graph_stats(&self) -> Result<GraphStats> {
        // graph_nodes 테이블이 없으면 빈 stats 반환
        let table_exists: i64 = self.conn().query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='graph_nodes'",
            [],
            |r| r.get(0),
        )?;
        if table_exists == 0 {
            return Ok(GraphStats::default());
        }

        let node_count: i64 =
            self.conn()
                .query_row("SELECT COUNT(*) FROM graph_nodes", [], |r| r.get(0))?;
        let edge_count: i64 =
            self.conn()
                .query_row("SELECT COUNT(*) FROM graph_edges", [], |r| r.get(0))?;

        let mut nodes_by_type: HashMap<String, usize> = HashMap::new();
        let mut stmt = self
            .conn()
            .prepare("SELECT type, COUNT(*) FROM graph_nodes GROUP BY type")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        for row in rows.filter_map(|r| r.ok()) {
            nodes_by_type.insert(row.0, row.1 as usize);
        }

        let mut edges_by_relation: HashMap<String, usize> = HashMap::new();
        let mut stmt = self
            .conn()
            .prepare("SELECT relation, COUNT(*) FROM graph_edges GROUP BY relation")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        for row in rows.filter_map(|r| r.ok()) {
            edges_by_relation.insert(row.0, row.1 as usize);
        }

        Ok(GraphStats {
            node_count: node_count as usize,
            edge_count: edge_count as usize,
            nodes_by_type,
            edges_by_relation,
        })
    }

    /// 전체 노드 목록 (type 필터 선택)
    /// 반환: Vec<(id, type, label)>
    pub fn list_graph_nodes(
        &self,
        node_type: Option<&str>,
    ) -> Result<Vec<(String, String, String)>> {
        if let Some(t) = node_type {
            let mut stmt = self
                .conn()
                .prepare("SELECT id, type, label FROM graph_nodes WHERE type = ?1")?;
            let rows: Vec<_> = stmt
                .query_map([t], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        } else {
            let mut stmt = self
                .conn()
                .prepare("SELECT id, type, label FROM graph_nodes")?;
            let rows: Vec<_> = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        }
    }

    /// 그래프 전체 초기화 (--force 용)
    pub fn clear_graph(&self) -> Result<()> {
        self.conn()
            .execute_batch("DELETE FROM graph_edges; DELETE FROM graph_nodes;")?;
        Ok(())
    }

    /// 특정 세션과 관련된 그래프 데이터 삭제 (증분 재빌드 용)
    pub fn delete_graph_for_session(&self, session_id: &str) -> Result<()> {
        let node_id = format!("session:{}", session_id);
        // 해당 세션 노드가 source/target인 엣지 삭제
        self.conn().execute(
            "DELETE FROM graph_edges WHERE source = ?1 OR target = ?1",
            rusqlite::params![node_id],
        )?;
        // 세션 노드 삭제
        self.conn().execute(
            "DELETE FROM graph_nodes WHERE id = ?1",
            rusqlite::params![node_id],
        )?;
        Ok(())
    }

    /// 이미 그래프에 포함된 세션 ID 목록
    pub fn list_graphed_session_ids(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn()
            .prepare("SELECT id FROM graph_nodes WHERE type = 'session'")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        // "session:{id}" → "{id}" 변환
        let ids = rows
            .filter_map(|r| r.ok())
            .filter_map(|s| s.strip_prefix("session:").map(|id| id.to_string()))
            .collect();
        Ok(ids)
    }

    /// 특정 relation 타입의 엣지를 전체 삭제.
    /// 증분 빌드에서 same_project/same_day를 전체 재계산할 때 사용.
    pub fn delete_relation_edges(&self, relations: &[&str]) -> Result<usize> {
        if relations.is_empty() {
            return Ok(0);
        }
        let placeholders: Vec<String> = (1..=relations.len()).map(|i| format!("?{}", i)).collect();
        let sql = format!(
            "DELETE FROM graph_edges WHERE relation IN ({})",
            placeholders.join(", ")
        );
        let params: Vec<&dyn rusqlite::types::ToSql> = relations
            .iter()
            .map(|r| r as &dyn rusqlite::types::ToSql)
            .collect();
        let deleted = self.conn().execute(&sql, params.as_slice())?;
        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use crate::store::Database;

    #[test]
    fn test_graph_upsert_and_stats() {
        let db = Database::open_memory().unwrap();

        // 노드 삽입
        db.upsert_graph_node("session:abc123", "session", "Session ABC", None)
            .unwrap();
        db.upsert_graph_node("project:tunaflow", "project", "tunaflow", None)
            .unwrap();
        db.upsert_graph_node("agent:claude-code", "agent", "claude-code", None)
            .unwrap();

        // 엣지 삽입
        db.upsert_graph_edge(
            "session:abc123",
            "project:tunaflow",
            "belongs_to",
            "EXTRACTED",
            1.0,
        )
        .unwrap();
        db.upsert_graph_edge(
            "session:abc123",
            "agent:claude-code",
            "by_agent",
            "EXTRACTED",
            1.0,
        )
        .unwrap();

        // 중복 엣지 — INSERT OR IGNORE이므로 무시
        db.upsert_graph_edge(
            "session:abc123",
            "project:tunaflow",
            "belongs_to",
            "EXTRACTED",
            1.0,
        )
        .unwrap();

        let stats = db.graph_stats().unwrap();
        assert_eq!(stats.node_count, 3);
        assert_eq!(stats.edge_count, 2);
        assert_eq!(stats.nodes_by_type.get("session"), Some(&1));
        assert_eq!(stats.nodes_by_type.get("project"), Some(&1));
        assert_eq!(stats.edges_by_relation.get("belongs_to"), Some(&1));
    }

    #[test]
    fn test_delete_relation_edges() {
        let db = Database::open_memory().unwrap();

        // same_project 엣지 3개 삽입
        db.upsert_graph_node("session:s1", "session", "S1", None)
            .unwrap();
        db.upsert_graph_node("session:s2", "session", "S2", None)
            .unwrap();
        db.upsert_graph_node("session:s3", "session", "S3", None)
            .unwrap();
        db.upsert_graph_node("project:p1", "project", "P1", None)
            .unwrap();

        db.upsert_graph_edge("session:s1", "session:s2", "same_project", "EXTRACTED", 1.0)
            .unwrap();
        db.upsert_graph_edge("session:s2", "session:s3", "same_project", "EXTRACTED", 1.0)
            .unwrap();
        db.upsert_graph_edge("session:s1", "session:s3", "same_project", "EXTRACTED", 1.0)
            .unwrap();
        // belongs_to 엣지 2개 삽입
        db.upsert_graph_edge("session:s1", "project:p1", "belongs_to", "EXTRACTED", 1.0)
            .unwrap();
        db.upsert_graph_edge("session:s2", "project:p1", "belongs_to", "EXTRACTED", 1.0)
            .unwrap();

        // same_project만 삭제
        let deleted = db.delete_relation_edges(&["same_project"]).unwrap();
        assert_eq!(deleted, 3);

        // same_project 0개, belongs_to 2개 확인
        let stats = db.graph_stats().unwrap();
        assert_eq!(stats.edges_by_relation.get("same_project"), None);
        assert_eq!(stats.edges_by_relation.get("belongs_to"), Some(&2));
    }

    #[test]
    fn test_graph_neighbors() {
        let db = Database::open_memory().unwrap();

        db.upsert_graph_node("session:s1", "session", "S1", None)
            .unwrap();
        db.upsert_graph_node("project:p1", "project", "P1", None)
            .unwrap();
        db.upsert_graph_node("tool:Edit", "tool", "Edit", None)
            .unwrap();

        db.upsert_graph_edge("session:s1", "project:p1", "belongs_to", "EXTRACTED", 1.0)
            .unwrap();
        db.upsert_graph_edge("session:s1", "tool:Edit", "uses_tool", "EXTRACTED", 1.0)
            .unwrap();

        // session:s1 이웃 — 2개 (나가는 방향)
        let neighbors = db.get_neighbors("session:s1").unwrap();
        assert_eq!(neighbors.len(), 2);
        assert!(neighbors
            .iter()
            .any(|(id, _, d)| id == "project:p1" && d == "out"));
        assert!(neighbors
            .iter()
            .any(|(id, _, d)| id == "tool:Edit" && d == "out"));

        // project:p1 이웃 — 1개 (들어오는 방향)
        let nb = db.get_neighbors("project:p1").unwrap();
        assert_eq!(nb.len(), 1);
        assert_eq!(nb[0].2, "in");
    }
}
