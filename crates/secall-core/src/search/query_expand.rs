use anyhow::Result;

/// Expand a search query using Claude Code CLI.
///
/// Calls `claude -p <prompt> --model claude-haiku-4-5-20251001` as a subprocess.
/// If `claude` is not in PATH or the call fails, returns the original query unchanged.
pub fn expand_query(query: &str) -> Result<String> {
    if !command_exists("claude") {
        tracing::warn!("claude not found, using original query");
        return Ok(query.to_string());
    }

    let prompt = format!(
        "다음 검색 쿼리를 확장해주세요. \
         원본 쿼리의 키워드, 동의어, 관련 기술 용어, 영어/한국어 변환을 포함하세요. \
         결과는 공백으로 구분된 키워드만 출력하세요. 설명 없이 키워드만.\n\n\
         쿼리: {query}"
    );

    let output = std::process::Command::new("claude")
        .args(["-p", &prompt, "--model", "claude-haiku-4-5-20251001"])
        .output()?;

    if output.status.success() {
        let expanded = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !expanded.is_empty() {
            tracing::info!(original = query, expanded = %expanded, "query expanded");
            Ok(format!("{query} {expanded}"))
        } else {
            Ok(query.to_string())
        }
    } else {
        tracing::warn!("query expansion failed, using original query");
        Ok(query.to_string())
    }
}

fn command_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_query_no_claude() {
        // If claude is not installed, should return original query
        if command_exists("claude") {
            // Claude is installed; skip this specific path test
            return;
        }
        let result = expand_query("벡터 검색").unwrap();
        assert_eq!(result, "벡터 검색");
    }

    #[test]
    fn test_command_exists_false() {
        assert!(!command_exists("__nonexistent_command_xyz__"));
    }

    #[test]
    #[ignore]
    fn test_expand_query_real() {
        // Manual: requires claude CLI installed
        let result = expand_query("벡터 검색").unwrap();
        assert!(result.contains("벡터 검색"), "original query should be included");
        assert!(result.len() > "벡터 검색".len(), "should have expanded terms");
    }
}
