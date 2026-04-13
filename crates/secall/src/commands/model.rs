use anyhow::Result;
use secall_core::search::model_manager::{default_model_path, ModelManager, UpdateStatus};

pub async fn run_download(force: bool) -> Result<()> {
    let model_dir = default_model_path();
    let mgr = ModelManager::new(model_dir);
    mgr.download(force).await
}

pub async fn run_check() -> Result<()> {
    let model_dir = default_model_path();
    let mgr = ModelManager::new(model_dir);
    match mgr.check_update().await? {
        UpdateStatus::UpToDate => {
            println!("✓ Model is up to date.");
        }
        UpdateStatus::NeedsUpdate { remote_modified } => {
            println!("⚠ Update available (remote lastModified: {remote_modified}).");
            println!("  Run: secall model download --force");
        }
        UpdateStatus::NotInstalled => {
            println!("⚠ Model not installed. Run: secall model download");
        }
        UpdateStatus::CheckFailed(reason) => {
            println!("⚠ Update check failed: {reason}");
        }
    }
    Ok(())
}

pub fn run_remove() -> Result<()> {
    let model_dir = default_model_path();
    let mgr = ModelManager::new(model_dir);
    mgr.remove()
}

pub fn run_info() -> Result<()> {
    let model_dir = default_model_path();
    let mgr = ModelManager::new(model_dir);
    let info = mgr.info()?;

    println!("Path:      {}", info.path.display());
    if let Some(ver) = &info.version {
        println!("Model:     {}", ver.model);
        println!("Revision:  {}", ver.source_revision);
        println!("Downloaded:{}", ver.downloaded_at);
    } else {
        println!("Version:   (no version.json)");
    }
    match (info.model_size, info.tokenizer_size) {
        (Some(m), Some(t)) => {
            let data_str = info
                .model_data_size
                .map(|d| format!(", model.onnx_data ({:.0}MB)", d as f64 / (1024.0 * 1024.0)))
                .unwrap_or_default();
            println!(
                "Files:     model.onnx ({:.0}MB){}, tokenizer.json ({:.0}KB)",
                m as f64 / (1024.0 * 1024.0),
                data_str,
                t as f64 / 1024.0
            );
        }
        _ => {
            println!("Files:     not downloaded");
        }
    }
    Ok(())
}
