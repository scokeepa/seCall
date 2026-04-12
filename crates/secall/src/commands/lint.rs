use anyhow::Result;
use secall_core::{
    ingest::lint::{run_lint, Severity},
    store::{get_default_db_path, Database},
    vault::Config,
};

pub fn run(json: bool, errors_only: bool, fix: bool) -> Result<()> {
    let config = Config::load_or_default();
    let db_path = get_default_db_path();
    let db = Database::open(&db_path)?;

    let report = run_lint(&db, &config)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        if fix {
            run_fix(&db, &report)?;
        }
        return Ok(());
    }

    // Text output
    println!("secall lint report");
    println!("==================");

    let mut printed = 0;
    for finding in &report.findings {
        if errors_only && !matches!(finding.severity, Severity::Error) {
            continue;
        }
        let sev = finding.severity.as_str();
        let sid = finding
            .session_id
            .as_deref()
            .map(|s| format!("session {}: ", &s[..s.len().min(8)]))
            .unwrap_or_default();
        println!("{} [{sev:5}] {sid}{}", finding.code, finding.message);
        printed += 1;
    }

    if printed == 0 {
        println!("No issues found.");
    }

    println!();
    println!(
        "Summary: {} sessions, {} errors, {} warnings, {} info",
        report.summary.total_sessions,
        report.summary.errors,
        report.summary.warnings,
        report.summary.info,
    );

    if !report.summary.agents.is_empty() {
        let agent_str: Vec<String> = {
            let mut pairs: Vec<_> = report.summary.agents.iter().collect();
            pairs.sort_by_key(|(k, _)| k.as_str());
            pairs.iter().map(|(k, v)| format!("{k}({v})")).collect()
        };
        println!("Agents: {}", agent_str.join(", "));
    }

    // --fix: auto-repair L001 (stale DB records)
    if fix {
        run_fix(&db, &report)?;
    }

    // Exit with code 1 if there are errors (after fix, re-count)
    let remaining_errors = if fix {
        // Re-run lint to get updated count
        let updated = run_lint(&db, &config)?;
        updated.summary.errors
    } else {
        report.summary.errors
    };

    if remaining_errors > 0 {
        std::process::exit(1);
    }

    Ok(())
}

fn run_fix(db: &Database, report: &secall_core::ingest::lint::LintReport) -> Result<()> {
    let stale: Vec<&str> = report
        .findings
        .iter()
        .filter(|f| f.code == "L001" && f.session_id.is_some())
        .filter(|f| f.message.contains("vault file missing"))
        .filter_map(|f| f.session_id.as_deref())
        .collect();

    if stale.is_empty() {
        eprintln!("[fix] No stale DB records to clean up.");
        return Ok(());
    }

    eprintln!(
        "[fix] Removing {} stale DB record(s) with missing vault files...",
        stale.len()
    );
    for session_id in &stale {
        match db.delete_session(session_id) {
            Ok(()) => eprintln!("  deleted {}", &session_id[..session_id.len().min(8)]),
            Err(e) => eprintln!(
                "  failed to delete {}: {e}",
                &session_id[..session_id.len().min(8)]
            ),
        }
    }
    eprintln!("[fix] Done. {} record(s) removed.", stale.len());
    Ok(())
}
