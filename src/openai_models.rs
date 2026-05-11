//! OpenAI model docs helpers for syncing the frontier model list.

use anyhow::{bail, Context, Result};
use std::collections::HashSet;
use std::path::Path;

pub const OPENAI_MODELS_ALL_URL: &str = "https://developers.openai.com/api/docs/models/all";
pub const FRONTIER_MODELS_BLOCK_START: &str = "// BEGIN OPENAI_FRONTIER_MODELS";
pub const FRONTIER_MODELS_BLOCK_END: &str = "// END OPENAI_FRONTIER_MODELS";

const FRONTIER_SECTION_START: &str = "id=\"frontier\"";
const FRONTIER_SECTION_END: &str = "id=\"image\"";
const MODEL_HREF_PREFIX: &str = "href=\"/api/docs/models/";
const OPENAI_FRONTIER_CONST_START: &str = "const OPENAI_FRONTIER_MODELS: &[&str] = &[";

pub fn fetch_frontier_model_ids() -> Result<Vec<String>> {
    let response = ureq::get(OPENAI_MODELS_ALL_URL)
        .header(
            "User-Agent",
            &format!("azureal/{}", env!("CARGO_PKG_VERSION")),
        )
        .call()
        .map_err(|e| anyhow::anyhow!("OpenAI docs fetch failed: {}", e))?;

    let html = response
        .into_body()
        .read_to_string()
        .map_err(|e| anyhow::anyhow!("OpenAI docs read failed: {}", e))?;

    parse_frontier_model_ids(&html)
}

pub fn parse_frontier_model_ids(html: &str) -> Result<Vec<String>> {
    let start = html
        .find(FRONTIER_SECTION_START)
        .context("Could not find OpenAI frontier models section")?;
    let rest = &html[start..];
    let end = rest
        .find(FRONTIER_SECTION_END)
        .context("Could not find end of OpenAI frontier models section")?;
    let section = &rest[..end];

    let mut models = Vec::new();
    let mut seen = HashSet::new();
    let mut cursor = section;

    while let Some(idx) = cursor.find(MODEL_HREF_PREFIX) {
        let after_prefix = &cursor[idx + MODEL_HREF_PREFIX.len()..];
        let slug_end = after_prefix
            .find('"')
            .context("Unterminated frontier model link in docs HTML")?;
        let slug = &after_prefix[..slug_end];
        if slug.is_empty() {
            bail!("Encountered empty frontier model slug in docs HTML");
        }
        if seen.insert(slug.to_string()) {
            models.push(slug.to_string());
        }
        cursor = &after_prefix[slug_end..];
    }

    validate_frontier_model_ids(&models)?;
    Ok(models)
}

pub fn render_synced_model_file(
    source: &str,
    models: &[String],
    synced_on: &str,
) -> Result<String> {
    validate_frontier_model_ids(models)?;
    replace_frontier_models_block(source, &render_frontier_models_block(models, synced_on))
}

pub fn extract_frontier_model_ids_from_model_source(source: &str) -> Result<Vec<String>> {
    let start = source
        .find(FRONTIER_MODELS_BLOCK_START)
        .context("Could not find OpenAI frontier block start marker")?;
    let end_marker = source
        .find(FRONTIER_MODELS_BLOCK_END)
        .context("Could not find OpenAI frontier block end marker")?;
    let end = end_marker + FRONTIER_MODELS_BLOCK_END.len();
    let block = &source[start..end];

    let const_start = block
        .find(OPENAI_FRONTIER_CONST_START)
        .context("Could not find OpenAI frontier const in model source")?;
    let after_const = &block[const_start + OPENAI_FRONTIER_CONST_START.len()..];
    let const_end = after_const
        .find("];")
        .context("Could not find end of OpenAI frontier const in model source")?;
    let body = &after_const[..const_end];

    let mut models = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let trimmed = trimmed
            .strip_suffix(',')
            .context("Malformed OpenAI frontier model entry in model source")?;
        let model = trimmed
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .context("Malformed OpenAI frontier model entry in model source")?;
        models.push(model.to_string());
    }

    validate_frontier_model_ids(&models)?;
    Ok(models)
}

pub fn sync_model_file(path: &Path, models: &[String], synced_on: &str) -> Result<bool> {
    let source = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let updated = render_synced_model_file(&source, models, synced_on)?;
    if updated == source {
        return Ok(false);
    }
    std::fs::write(path, updated).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(true)
}

fn validate_frontier_model_ids(models: &[String]) -> Result<()> {
    if models.is_empty() {
        bail!("OpenAI frontier model list is empty");
    }
    for model in models {
        if !model.starts_with("gpt-") {
            bail!(
                "Unsupported frontier model id '{}' from OpenAI docs; Azureal only classifies gpt-* models as Codex",
                model
            );
        }
    }
    Ok(())
}

fn render_frontier_models_block(models: &[String], synced_on: &str) -> String {
    let mut block = String::new();
    block.push_str(FRONTIER_MODELS_BLOCK_START);
    block.push('\n');
    block.push_str("/// OpenAI frontier models in docs order.\n");
    block.push_str(&format!(
        "/// Sourced from the OpenAI docs \"Frontier models\" section on {}.\n",
        synced_on
    ));
    block.push_str("/// `azureal models sync-openai-frontier` rewrites this block.\n");
    block.push_str("const OPENAI_FRONTIER_MODELS: &[&str] = &[\n");
    for model in models {
        block.push_str(&format!("    \"{}\",\n", model));
    }
    block.push_str("];\n");
    block.push_str(FRONTIER_MODELS_BLOCK_END);
    block
}

fn replace_frontier_models_block(source: &str, block: &str) -> Result<String> {
    let start = source
        .find(FRONTIER_MODELS_BLOCK_START)
        .context("Could not find OpenAI frontier block start marker")?;
    let end_marker = source
        .find(FRONTIER_MODELS_BLOCK_END)
        .context("Could not find OpenAI frontier block end marker")?;
    let end = end_marker + FRONTIER_MODELS_BLOCK_END.len();
    if end <= start {
        bail!("Invalid OpenAI frontier block marker order");
    }

    let mut updated = String::with_capacity(source.len() + block.len());
    updated.push_str(&source[..start]);
    updated.push_str(block);
    updated.push_str(&source[end..]);
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_HTML: &str = r#"
        <div id="frontier">
          <a href="/api/docs/models/gpt-5.5">GPT-5.5</a>
          <a href="/api/docs/models/gpt-5.4">GPT-5.4</a>
          <a href="/api/docs/models/gpt-5-mini">GPT-5 mini</a>
        </div>
        <div id="image">
          <a href="/api/docs/models/gpt-image-2">GPT Image 2</a>
        </div>
    "#;

    #[test]
    fn test_parse_frontier_model_ids_extracts_frontier_only() {
        let models = parse_frontier_model_ids(SAMPLE_HTML).unwrap();
        assert_eq!(models, vec!["gpt-5.5", "gpt-5.4", "gpt-5-mini"]);
    }

    #[test]
    fn test_parse_frontier_model_ids_dedupes_repeated_links() {
        let html = r#"
            <div id="frontier">
              <a href="/api/docs/models/gpt-5.5">GPT-5.5</a>
              <a href="/api/docs/models/gpt-5.5">GPT-5.5</a>
            </div>
            <div id="image"></div>
        "#;
        let models = parse_frontier_model_ids(html).unwrap();
        assert_eq!(models, vec!["gpt-5.5"]);
    }

    #[test]
    fn test_parse_frontier_model_ids_rejects_non_gpt_models() {
        let html = r#"
            <div id="frontier">
              <a href="/api/docs/models/o3">o3</a>
            </div>
            <div id="image"></div>
        "#;
        let err = parse_frontier_model_ids(html).unwrap_err().to_string();
        assert!(err.contains("Unsupported frontier model id 'o3'"));
    }

    #[test]
    fn test_render_synced_model_file_replaces_marker_block() {
        let source = r#"
before
// BEGIN OPENAI_FRONTIER_MODELS
old
// END OPENAI_FRONTIER_MODELS
after
"#;
        let updated = render_synced_model_file(
            source,
            &[String::from("gpt-5.5"), String::from("gpt-5.4")],
            "2026-05-11",
        )
        .unwrap();
        assert!(updated.contains("const OPENAI_FRONTIER_MODELS: &[&str] = &["));
        assert!(updated.contains("\"gpt-5.5\""));
        assert!(updated.contains("\"gpt-5.4\""));
        assert!(updated.contains("on 2026-05-11."));
        assert!(updated.contains("before"));
        assert!(updated.contains("after"));
        assert!(!updated.contains("\nold\n"));
    }

    #[test]
    fn test_render_synced_model_file_requires_markers() {
        let err =
            render_synced_model_file("missing markers", &[String::from("gpt-5.5")], "2026-05-11")
                .unwrap_err()
                .to_string();
        assert!(err.contains("block start marker"));
    }

    #[test]
    fn test_extract_frontier_model_ids_from_model_source_reads_synced_block() {
        let source = render_synced_model_file(
            r#"
before
// BEGIN OPENAI_FRONTIER_MODELS
old
// END OPENAI_FRONTIER_MODELS
after
"#,
            &[String::from("gpt-5.5"), String::from("gpt-5.4")],
            "2026-05-11",
        )
        .unwrap();
        let models = extract_frontier_model_ids_from_model_source(&source).unwrap();
        assert_eq!(models, vec!["gpt-5.5", "gpt-5.4"]);
    }
}
