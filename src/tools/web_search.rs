use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::fmt::Write;

/// Web search tool using the Brave Search API.
pub struct WebSearchTool {
    api_key: String,
    count: u8,
}

#[derive(Debug, Deserialize)]
struct BraveSearchResponse {
    web: Option<BraveWebResults>,
}

#[derive(Debug, Deserialize)]
struct BraveWebResults {
    results: Vec<BraveSearchResult>,
}

#[derive(Debug, Deserialize)]
struct BraveSearchResult {
    title: String,
    url: String,
    description: Option<String>,
    age: Option<String>,
}

impl WebSearchTool {
    pub fn new(api_key: &str, count: u8) -> Self {
        Self {
            api_key: api_key.to_string(),
            count: count.clamp(1, 20),
        }
    }
}

#[allow(clippy::too_many_lines)]
#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the web using Brave Search. Returns titles, URLs, and snippets for the top results. \
        Use when you need current information, facts, documentation, or any knowledge beyond your training data."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query"
                },
                "count": {
                    "type": "integer",
                    "description": "Number of results to return (1-20, default from config)",
                    "minimum": 1,
                    "maximum": 20
                },
                "freshness": {
                    "type": "string",
                    "enum": ["pd", "pw", "pm", "py"],
                    "description": "Time filter: pd=past day, pw=past week, pm=past month, py=past year"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' parameter"))?;

        if query.trim().is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Search query cannot be empty".into()),
            });
        }

        let count = args
            .get("count")
            .and_then(serde_json::Value::as_u64)
            .map_or(self.count, |c| u8::try_from(c).unwrap_or(20).clamp(1, 20));

        let freshness = args
            .get("freshness")
            .and_then(|v| v.as_str())
            .map(String::from);

        // Build query parameters
        let mut params: Vec<(&str, String)> =
            vec![("q", query.to_string()), ("count", count.to_string())];
        if let Some(ref f) = freshness {
            params.push(("freshness", f.clone()));
        }

        // Make the API call
        let client = reqwest::Client::new();
        let response = client
            .get("https://api.search.brave.com/res/v1/web/search")
            .query(&params)
            .header("Accept", "application/json")
            .header("X-Subscription-Token", &self.api_key)
            .send()
            .await;

        let response = match response {
            Ok(r) => r,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Brave Search request failed: {e}")),
                });
            }
        };

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Brave Search API error ({status}): {body}")),
            });
        }

        let text = match response.text().await {
            Ok(t) => t,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to read Brave Search response: {e}")),
                });
            }
        };

        let body = match serde_json::from_str::<BraveSearchResponse>(&text) {
            Ok(b) => b,
            Err(e) => {
                let preview = if text.len() > 200 {
                    &text[..200]
                } else {
                    &text
                };
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Failed to parse Brave Search response: {e}\nBody preview: {preview}"
                    )),
                });
            }
        };

        let results = body.web.map(|w| w.results).unwrap_or_default();

        if results.is_empty() {
            return Ok(ToolResult {
                success: true,
                output: format!("No results found for: {query}"),
                error: None,
            });
        }

        // Format results as readable text
        let mut output = String::new();
        for (i, r) in results.iter().enumerate() {
            let _ = writeln!(output, "{}. {}", i + 1, r.title);
            let _ = writeln!(output, "   {}", r.url);
            if let Some(ref desc) = r.description {
                let _ = writeln!(output, "   {desc}");
            }
            if let Some(ref age) = r.age {
                let _ = writeln!(output, "   ({age})");
            }
            output.push('\n');
        }

        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_name_and_description() {
        let tool = WebSearchTool::new("test-key", 5);
        assert_eq!(tool.name(), "web_search");
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn parameters_schema_has_query() {
        let tool = WebSearchTool::new("test-key", 5);
        let schema = tool.parameters_schema();
        assert!(schema["properties"]["query"].is_object());
        assert_eq!(schema["required"][0], "query");
    }

    #[test]
    fn count_clamped() {
        let tool = WebSearchTool::new("key", 50);
        assert_eq!(tool.count, 20);
        let tool = WebSearchTool::new("key", 0);
        assert_eq!(tool.count, 1);
    }

    #[tokio::test]
    async fn empty_query_returns_error() {
        let tool = WebSearchTool::new("key", 5);
        let result = tool.execute(json!({"query": "  "})).await.unwrap();
        assert!(!result.success);
        assert!(result.error.unwrap().contains("empty"));
    }

    #[tokio::test]
    async fn missing_query_returns_error() {
        let tool = WebSearchTool::new("key", 5);
        let result = tool.execute(json!({})).await;
        assert!(result.is_err());
    }
}
