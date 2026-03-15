//! Web search tool -- queries Brave Search API.
//!
//! Implements the Rig `Tool` trait for performing web searches via the
//! Brave Search API, returning structured results with titles, URLs,
//! and snippets.

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::Deserialize;

use crate::tools::truncate_output;

/// Maximum output size in bytes before truncation (100KB).
const MAX_OUTPUT_BYTES: usize = 102_400;

/// Default number of search results to return.
const DEFAULT_RESULT_COUNT: u32 = 10;

/// Brave Search API endpoint.
const BRAVE_SEARCH_URL: &str = "https://api.search.brave.com/res/v1/web/search";

/// Arguments for the web search tool, deserialized from AI-provided JSON.
#[derive(Debug, Deserialize)]
pub struct WebSearchArgs {
    /// The search query.
    pub query: String,
    /// Number of results to return (default: 10).
    #[serde(default)]
    pub count: Option<u32>,
}

/// Errors that can occur during web search execution.
#[derive(Debug, thiserror::Error)]
pub enum WebSearchError {
    /// API key not configured.
    #[error("Brave Search API key not configured. Add brave_api_key to ~/.seval/config.toml")]
    NoApiKey,
    /// HTTP request failed.
    #[error("Search request failed: {0}")]
    RequestError(String),
    /// Failed to parse API response.
    #[error("Failed to parse search response: {0}")]
    ParseError(String),
}

/// Brave Search API response structure.
#[derive(Debug, Deserialize)]
struct BraveResponse {
    /// Web search results section.
    #[serde(default)]
    web: Option<BraveWeb>,
}

/// Web results section of the Brave Search response.
#[derive(Debug, Deserialize)]
struct BraveWeb {
    /// Individual search results.
    #[serde(default)]
    results: Vec<BraveResult>,
}

/// Individual search result from Brave Search.
#[derive(Debug, Deserialize)]
struct BraveResult {
    /// Result title.
    #[serde(default)]
    title: String,
    /// Result URL.
    #[serde(default)]
    url: String,
    /// Result description/snippet.
    #[serde(default)]
    description: String,
}

/// Web search tool for querying the Brave Search API.
///
/// Performs web searches and returns structured results with titles,
/// URLs, and snippets formatted as a numbered list.
pub struct WebSearchTool {
    /// Brave Search API key (None if not configured).
    api_key: Option<String>,
}

impl WebSearchTool {
    /// Create a new `WebSearchTool` with an optional API key.
    pub fn new(api_key: Option<String>) -> Self {
        Self { api_key }
    }
}

impl Tool for WebSearchTool {
    const NAME: &'static str = "web_search";

    type Error = WebSearchError;
    type Args = WebSearchArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "web_search".to_string(),
            description: "Search the web using Brave Search API. Returns structured results \
                          with titles, URLs, and descriptions. Requires a Brave Search API key \
                          configured in ~/.seval/config.toml."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query"
                    },
                    "count": {
                        "type": "integer",
                        "description": "Number of results to return (default: 10, max: 20)"
                    }
                },
                "required": ["query"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let api_key = self
            .api_key
            .as_ref()
            .ok_or(WebSearchError::NoApiKey)?;

        let count = args.count.unwrap_or(DEFAULT_RESULT_COUNT).min(20);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| WebSearchError::RequestError(e.to_string()))?;

        let response = client
            .get(BRAVE_SEARCH_URL)
            .header("X-Subscription-Token", api_key)
            .header("Accept", "application/json")
            .query(&[
                ("q", args.query.as_str()),
                ("count", &count.to_string()),
            ])
            .send()
            .await
            .map_err(|e| WebSearchError::RequestError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(WebSearchError::RequestError(format!(
                "HTTP {}",
                response.status()
            )));
        }

        let brave_response: BraveResponse = response
            .json()
            .await
            .map_err(|e| WebSearchError::ParseError(e.to_string()))?;

        let results = brave_response
            .web
            .map(|w| w.results)
            .unwrap_or_default();

        if results.is_empty() {
            return Ok("No results found.".to_string());
        }

        let formatted = format_results(&results);
        Ok(truncate_output(&formatted, MAX_OUTPUT_BYTES))
    }
}

/// Format search results as a numbered list.
fn format_results(results: &[BraveResult]) -> String {
    use std::fmt::Write;
    let mut output = String::new();
    for (i, result) in results.iter().enumerate() {
        if i > 0 {
            output.push('\n');
        }
        let _ = write!(
            output,
            "{}. {}\n   {}\n   {}",
            i + 1,
            result.title,
            result.url,
            result.description
        );
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_definition_has_required_fields() {
        let tool = WebSearchTool::new(None);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let def = rt.block_on(tool.definition(String::new()));

        assert_eq!(def.name, "web_search");
        assert!(!def.description.is_empty());

        let params = &def.parameters;
        assert_eq!(params["type"], "object");
        assert!(params["properties"]["query"].is_object());
        assert!(params["properties"]["count"].is_object());

        let required = params["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "query"));
    }

    #[tokio::test]
    async fn test_no_api_key_returns_error() {
        let tool = WebSearchTool::new(None);
        let result = tool
            .call(WebSearchArgs {
                query: "test query".to_string(),
                count: None,
            })
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Brave Search API key not configured"),
            "got: {err}"
        );
        assert!(err.contains("config.toml"), "should mention config file, got: {err}");
    }

    #[test]
    fn test_result_formatting_single() {
        let results = vec![BraveResult {
            title: "Example Page".to_string(),
            url: "https://example.com".to_string(),
            description: "An example page for testing".to_string(),
        }];

        let formatted = format_results(&results);
        assert!(formatted.contains("1. Example Page"));
        assert!(formatted.contains("https://example.com"));
        assert!(formatted.contains("An example page for testing"));
    }

    #[test]
    fn test_result_formatting_multiple() {
        let results = vec![
            BraveResult {
                title: "First".to_string(),
                url: "https://first.com".to_string(),
                description: "First result".to_string(),
            },
            BraveResult {
                title: "Second".to_string(),
                url: "https://second.com".to_string(),
                description: "Second result".to_string(),
            },
            BraveResult {
                title: "Third".to_string(),
                url: "https://third.com".to_string(),
                description: "Third result".to_string(),
            },
        ];

        let formatted = format_results(&results);
        assert!(formatted.contains("1. First"));
        assert!(formatted.contains("2. Second"));
        assert!(formatted.contains("3. Third"));
    }

    #[test]
    fn test_result_formatting_empty() {
        let results: Vec<BraveResult> = vec![];
        let formatted = format_results(&results);
        assert!(formatted.is_empty());
    }
}
