use anyhow::{Context, Result};

pub fn log_static_text(rec: &rerun::RecordingStream, entity_path: &str, text: &str) -> Result<()> {
    rec.log_static(entity_path, &rerun::TextDocument::from_markdown(text))
        .with_context(|| format!("static text log failed at {entity_path}"))?;
    Ok(())
}
