use anyhow::Result;
use secall_core::{
    store::{get_default_db_path, Database},
    vault::Config,
};

pub async fn run(_all: bool) -> Result<()> {
    let config = Config::load_or_default();
    let db_path = get_default_db_path();
    let _db = Database::open(&db_path)?;

    let vector_indexer = secall_core::search::vector::create_vector_indexer(&config).await;
    if vector_indexer.is_none() {
        tracing::warn!("no embedding backend available, check config [embedding] section");
        return Ok(());
    }

    println!("Vector embedding not yet fully implemented in this CLI path.");
    println!("Sessions are embedded automatically during `secall ingest`.");

    Ok(())
}
