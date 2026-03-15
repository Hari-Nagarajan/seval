//! Web fetch tool -- fetches URLs with HTML-to-text conversion.
//!
//! Implements the Rig `Tool` trait for fetching web pages, converting HTML
//! to readable text, and truncating output to a configurable size limit.

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::Deserialize;

use crate::tools::truncate_output;

/// Maximum output size in bytes before truncation (100KB).
const DEFAULT_MAX_BYTES: usize = 102_400;

/// Request timeout in seconds.
const REQUEST_TIMEOUT_SECS: u64 = 30;

/// Maximum number of redirects to follow.
const MAX_REDIRECTS: usize = 5;

/// Arguments for the web fetch tool, deserialized from AI-provided JSON.
#[derive(Debug, Deserialize)]
pub struct WebFetchArgs {
    /// The URL to fetch.
    pub url: String,
    /// Maximum output size in bytes (defaults to 100KB).
    #[serde(default)]
    pub max_bytes: Option<usize>,
}

/// Errors that can occur during web fetch execution.
#[derive(Debug, thiserror::Error)]
pub enum WebFetchError {
    /// HTTP request failed.
    #[error("HTTP request failed: {0}")]
    RequestError(String),
    /// Invalid URL.
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
    /// HTML conversion error.
    #[error("HTML conversion error: {0}")]
    ConversionError(String),
}

/// Web fetch tool for retrieving and converting web page content.
///
/// Fetches URLs via HTTP GET, converts HTML to readable text using
/// `html2text`, and truncates output to respect size limits.
pub struct WebFetchTool;

impl Default for WebFetchTool {
    fn default() -> Self {
        Self
    }
}

impl WebFetchTool {
    /// Create a new `WebFetchTool`.
    pub fn new() -> Self {
        Self
    }
}

impl Tool for WebFetchTool {
    const NAME: &'static str = "web_fetch";

    type Error = WebFetchError;
    type Args = WebFetchArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "web_fetch".to_string(),
            description: "Fetch a web page and return its content as text. HTML pages are \
                          automatically converted to readable text with headings, lists, and \
                          links preserved. Non-HTML content is returned as-is."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to fetch (must be http:// or https://)"
                    },
                    "max_bytes": {
                        "type": "integer",
                        "description": "Maximum output size in bytes (default: 102400 = 100KB)"
                    }
                },
                "required": ["url"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let max_bytes = args.max_bytes.unwrap_or(DEFAULT_MAX_BYTES);

        // Validate URL.
        if !args.url.starts_with("http://") && !args.url.starts_with("https://") {
            return Err(WebFetchError::InvalidUrl(format!(
                "URL must start with http:// or https://, got: {}",
                args.url
            )));
        }

        // Build reqwest client with timeout and redirect limit.
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .redirect(reqwest::redirect::Policy::limited(MAX_REDIRECTS))
            .build()
            .map_err(|e| WebFetchError::RequestError(e.to_string()))?;

        let response = client
            .get(&args.url)
            .send()
            .await
            .map_err(|e| WebFetchError::RequestError(e.to_string()))?;

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();

        let body = response
            .bytes()
            .await
            .map_err(|e| WebFetchError::RequestError(e.to_string()))?;

        let text = if content_type.contains("text/html") {
            html2text::from_read(&body[..], 80)
                .map_err(|e| WebFetchError::ConversionError(e.to_string()))?
        } else {
            String::from_utf8_lossy(&body).into_owned()
        };

        Ok(truncate_output(&text, max_bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_definition_has_required_fields() {
        let tool = WebFetchTool::new();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let def = rt.block_on(tool.definition(String::new()));

        assert_eq!(def.name, "web_fetch");
        assert!(!def.description.is_empty());

        let params = &def.parameters;
        assert_eq!(params["type"], "object");
        assert!(params["properties"]["url"].is_object());
        assert!(params["properties"]["max_bytes"].is_object());

        let required = params["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "url"));
    }

    #[test]
    fn test_html_to_text_conversion() {
        // Test the html2text conversion logic directly.
        let html = b"<html><body><h1>Title</h1><p>Hello <b>world</b></p><ul><li>Item 1</li><li>Item 2</li></ul></body></html>";
        let text = html2text::from_read(&html[..], 80).unwrap();

        assert!(text.contains("Title"), "should contain heading text");
        assert!(text.contains("Hello"), "should contain paragraph text");
        assert!(text.contains("world"), "should contain bold text as plain text");
        assert!(text.contains("Item 1"), "should contain list items");
        assert!(text.contains("Item 2"), "should contain list items");
    }

    #[test]
    fn test_html_links_preserved_as_text() {
        let html = b"<html><body><a href=\"https://example.com\">Click here</a></body></html>";
        let text = html2text::from_read(&html[..], 80).unwrap();
        assert!(
            text.contains("Click here"),
            "should contain link text"
        );
    }

    #[tokio::test]
    async fn test_invalid_url_returns_error() {
        let tool = WebFetchTool::new();
        let result = tool
            .call(WebFetchArgs {
                url: "not-a-url".to_string(),
                max_bytes: None,
            })
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("URL must start with http"), "got: {err}");
    }

    #[tokio::test]
    async fn test_ftp_url_returns_error() {
        let tool = WebFetchTool::new();
        let result = tool
            .call(WebFetchArgs {
                url: "ftp://example.com/file".to_string(),
                max_bytes: None,
            })
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn test_truncation_applied() {
        // Verify truncate_output works for web fetch output.
        let big_text = "x".repeat(200_000);
        let result = truncate_output(&big_text, DEFAULT_MAX_BYTES);
        assert!(
            result.len() <= DEFAULT_MAX_BYTES,
            "result len {} > {}",
            result.len(),
            DEFAULT_MAX_BYTES
        );
    }
}
