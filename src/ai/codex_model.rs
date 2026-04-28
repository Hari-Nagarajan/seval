//! Codex responses API completion model.
//!
//! Implements rig's `CompletionModel` trait against the `ChatGPT` backend API
//! (`chatgpt.com/backend-api/codex/responses`), enabling use with a `ChatGPT`
//! Pro/Plus subscription via Codex CLI auth tokens.

use std::pin::Pin;

use async_stream::stream;
use futures::Stream;
use rig::completion::GetTokenUsage;
use rig::completion::{
    CompletionError, CompletionModel, CompletionRequest, CompletionResponse, Usage,
};
use rig::message::{AssistantContent, Message, Text, ToolCall, UserContent};
use rig::streaming::{RawStreamingChoice, RawStreamingToolCall, StreamingCompletionResponse};
use serde::{Deserialize, Serialize};

use super::codex_auth::CodexAuth;

const DEFAULT_CODEX_URL: &str = "https://chatgpt.com/backend-api/codex/responses";

/// Client wrapper (required by `CompletionModel::Client`).
#[derive(Clone, Debug)]
pub struct CodexClient {
    pub auth: CodexAuth,
    pub base_url: String,
}

impl CodexClient {
    pub fn new(auth: CodexAuth) -> Self {
        let base_url =
            std::env::var("SEVAL_CODEX_URL").unwrap_or_else(|_| DEFAULT_CODEX_URL.to_string());
        Self { auth, base_url }
    }
}

/// Cache of call_id → (tool name, arguments JSON) for reconstructing
/// function_call items that rig's multi-turn may omit from history.
type ToolCallCache =
    std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, (String, String)>>>;

/// The completion model for the Codex responses API.
#[derive(Clone, Debug)]
pub struct CodexCompletionModel {
    client: CodexClient,
    model: String,
    http: reqwest::Client,
    tool_call_cache: ToolCallCache,
}

impl CodexCompletionModel {
    pub fn new(client: CodexClient, model: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .read_timeout(std::time::Duration::from_mins(5))
            .build()
            .expect("failed to build reqwest client");
        Self {
            client,
            model: normalize_model_id(&model.into()),
            http,
            tool_call_cache: std::sync::Arc::new(std::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
        }
    }
}

/// Streaming final response — carries token usage.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CodexStreamingResponse {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl rig::completion::GetTokenUsage for CodexStreamingResponse {
    fn token_usage(&self) -> Option<Usage> {
        Some(Usage {
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            total_tokens: self.input_tokens + self.output_tokens,
            cached_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }
}

/// Non-streaming response type.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CodexResponse {
    pub text: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

fn normalize_model_id(model: &str) -> String {
    if let Some(pos) = model.rfind('/') {
        model[pos + 1..].to_string()
    } else {
        model.to_string()
    }
}

impl CompletionModel for CodexCompletionModel {
    type Response = CodexResponse;
    type StreamingResponse = CodexStreamingResponse;
    type Client = CodexClient;

    fn make(client: &Self::Client, model: impl Into<String>) -> Self {
        Self::new(client.clone(), model)
    }

    async fn completion(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse<Self::Response>, CompletionError> {
        use futures::StreamExt;
        use rig::streaming::StreamedAssistantContent;

        let mut stream_resp = self.stream(request).await?;
        let mut text = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut usage = Usage {
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            cached_input_tokens: 0,
            cache_creation_input_tokens: 0,
        };

        while let Some(item) = stream_resp.next().await {
            match item? {
                StreamedAssistantContent::Text(t) => text.push_str(&t.text),
                StreamedAssistantContent::ToolCall { tool_call, .. } => {
                    tool_calls.push(ToolCall {
                        id: tool_call.id,
                        call_id: tool_call.call_id,
                        function: tool_call.function,
                        signature: None,
                        additional_params: None,
                    });
                }
                StreamedAssistantContent::Final(ref resp) => {
                    if let Some(u) = resp.token_usage() {
                        usage = u;
                    }
                }
                _ => {}
            }
        }

        let mut content = vec![AssistantContent::Text(Text { text: text.clone() })];
        for tc in tool_calls {
            content.push(AssistantContent::ToolCall(tc));
        }

        let choice = rig::one_or_many::OneOrMany::many(content).unwrap_or_else(|_| {
            rig::one_or_many::OneOrMany::one(AssistantContent::Text(Text {
                text: String::new(),
            }))
        });

        Ok(CompletionResponse {
            choice,
            usage,
            raw_response: CodexResponse {
                text,
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
            },
            message_id: None,
        })
    }

    async fn stream(
        &self,
        request: CompletionRequest,
    ) -> Result<StreamingCompletionResponse<Self::StreamingResponse>, CompletionError> {
        let creds = self
            .client
            .auth
            .credentials()
            .await
            .map_err(|e| CompletionError::ProviderError(e.to_string()))?;

        let body = build_request_body(&self.model, &request, &self.tool_call_cache);

        let resp = self
            .http
            .post(&self.client.base_url)
            .header("Authorization", format!("Bearer {}", creds.access_token))
            .header("OpenAI-Beta", "responses=experimental")
            .header("originator", "pi")
            .header("accept", "text/event-stream")
            .header("Content-Type", "application/json")
            .header("chatgpt-account-id", &creds.account_id)
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| CompletionError::ProviderError(format!("HTTP request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            return Err(CompletionError::ProviderError(format!(
                "Codex API error ({status}): {body_text}"
            )));
        }

        let byte_stream = resp.bytes_stream();
        let sse_stream = parse_sse_stream(byte_stream, self.tool_call_cache.clone());

        Ok(StreamingCompletionResponse::stream(sse_stream))
    }
}

/// Build the JSON request body for the Codex responses API.
fn build_request_body(
    model: &str,
    request: &CompletionRequest,
    tool_call_cache: &ToolCallCache,
) -> serde_json::Value {
    let mut system_parts: Vec<String> = Vec::new();
    let mut input_items: Vec<serde_json::Value> = Vec::new();

    // Extract system prompt from preamble.
    if let Some(ref preamble) = request.preamble {
        system_parts.push(preamble.clone());
    }

    // Convert all messages (chat_history includes the prompt as the last message).
    for msg in request.chat_history.iter() {
        convert_message(msg, &mut input_items, &mut system_parts);
    }

    // Ensure every function_call_output has a preceding function_call.
    // Rig 0.34's multi-turn can drop the Assistant message containing
    // function_call items from history. Reconstruct from the cache.
    let input_items = ensure_function_calls_present(input_items, tool_call_cache);

    let instructions = if system_parts.is_empty() {
        "You are Seval, a security research assistant.".to_string()
    } else {
        system_parts.join("\n\n")
    };

    // Codex responses API: tools have name/description/parameters at the top
    // level (not nested under "function" like the chat completions API).
    let tools: Vec<serde_json::Value> = request
        .tools
        .iter()
        .filter(|t| !t.name.is_empty())
        .map(|t| {
            serde_json::json!({
                "type": "function",
                "name": t.name,
                "description": t.description,
                "parameters": t.parameters,
            })
        })
        .collect();

    let mut body = serde_json::json!({
        "model": model,
        "input": input_items,
        "instructions": instructions,
        "store": false,
        "stream": true,
    });

    if !tools.is_empty() {
        body["tools"] = serde_json::json!(tools);
        body["tool_choice"] = serde_json::json!("auto");
    }

    body
}

/// Convert a rig `Message` into Codex responses API input items.
fn convert_message(
    msg: &Message,
    items: &mut Vec<serde_json::Value>,
    system_parts: &mut Vec<String>,
) {
    match msg {
        Message::System { content } => {
            system_parts.push(content.clone());
        }
        Message::User { content } => {
            let mut parts: Vec<serde_json::Value> = Vec::new();
            for item in content.iter() {
                match item {
                    UserContent::Text(t) => {
                        parts.push(serde_json::json!({
                            "type": "input_text",
                            "text": t.text,
                        }));
                    }
                    UserContent::ToolResult(tr) => {
                        // Codex responses API: tool results as function_call_output items.
                        let text: String = tr
                            .content
                            .iter()
                            .filter_map(|c| {
                                if let rig::message::ToolResultContent::Text(t) = c {
                                    Some(t.text.clone())
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        let cid = tr.call_id.as_deref().unwrap_or(&tr.id);
                        items.push(serde_json::json!({
                            "type": "function_call_output",
                            "call_id": cid,
                            "output": text,
                        }));
                        return;
                    }
                    _ => {}
                }
            }
            if !parts.is_empty() {
                items.push(serde_json::json!({
                    "role": "user",
                    "content": parts,
                }));
            }
        }
        Message::Assistant { content, .. } => {
            let mut text_parts: Vec<serde_json::Value> = Vec::new();
            let mut fn_calls: Vec<serde_json::Value> = Vec::new();
            for item in content.iter() {
                match item {
                    AssistantContent::Text(t) => {
                        text_parts.push(serde_json::json!({
                            "type": "output_text",
                            "text": t.text,
                        }));
                    }
                    AssistantContent::ToolCall(tc) => {
                        let cid = tc.call_id.as_deref().unwrap_or(&tc.id);
                        let fc_id = if cid.starts_with("fc_") {
                            cid.to_string()
                        } else {
                            format!("fc_{}", cid.trim_start_matches("call_"))
                        };
                        fn_calls.push(serde_json::json!({
                            "type": "function_call",
                            "id": fc_id,
                            "call_id": cid,
                            "name": tc.function.name,
                            "arguments": tc.function.arguments.to_string(),
                        }));
                    }
                    _ => {}
                }
            }
            // Responses API: assistant text must precede function_call items.
            if !text_parts.is_empty() {
                items.push(serde_json::json!({
                    "role": "assistant",
                    "content": text_parts,
                }));
            }
            items.extend(fn_calls);
        }
    }
}

/// Scan input items for `function_call_output` without a preceding `function_call`
/// and insert reconstructed function_call items from the cache.
fn ensure_function_calls_present(
    mut items: Vec<serde_json::Value>,
    cache: &ToolCallCache,
) -> Vec<serde_json::Value> {
    // Collect call_ids that have a function_call already present.
    let existing: std::collections::HashSet<String> = items
        .iter()
        .filter(|item| {
            item.get("type").and_then(serde_json::Value::as_str) == Some("function_call")
        })
        .filter_map(|item| {
            item.get("call_id")
                .and_then(serde_json::Value::as_str)
                .map(String::from)
        })
        .collect();

    let guard = cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    // Find function_call_output items missing a matching function_call.
    let mut insertions: Vec<(usize, serde_json::Value)> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        if item.get("type").and_then(serde_json::Value::as_str) != Some("function_call_output") {
            continue;
        }
        let Some(call_id) = item.get("call_id").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if existing.contains(call_id) {
            continue;
        }
        if let Some((name, arguments)) = guard.get(call_id) {
            // Codex API requires function_call `id` to start with "fc_".
            let fc_id = if call_id.starts_with("fc_") {
                call_id.to_string()
            } else {
                format!("fc_{}", call_id.trim_start_matches("call_"))
            };
            insertions.push((
                i,
                serde_json::json!({
                    "type": "function_call",
                    "id": fc_id,
                    "call_id": call_id,
                    "name": name,
                    "arguments": arguments,
                }),
            ));
        }
    }

    drop(guard);

    // Insert in reverse order to preserve indices.
    for (i, fc) in insertions.into_iter().rev() {
        items.insert(i, fc);
    }

    items
}

type SseStream = Pin<
    Box<
        dyn Stream<Item = Result<RawStreamingChoice<CodexStreamingResponse>, CompletionError>>
            + Send,
    >,
>;

/// Parse a byte stream of SSE events into `RawStreamingChoice` items.
fn parse_sse_stream<S>(byte_stream: S, tool_call_cache: ToolCallCache) -> SseStream
where
    S: Stream<Item = Result<::bytes::Bytes, reqwest::Error>> + Send + 'static,
{
    use futures::StreamExt;

    Box::pin(stream! {
        let mut byte_stream = std::pin::pin!(byte_stream);
        let mut buffer = String::new();
        let mut accumulated_text = String::new();
        let mut done_text: Option<String> = None;
        // Track function call metadata from output_item.added events,
        // keyed by output_index. The done event often omits `name`.
        let mut fn_call_meta: std::collections::HashMap<u64, (String, String)> =
            std::collections::HashMap::new();

        while let Some(chunk_result) = byte_stream.next().await {
            let chunk = match chunk_result {
                Ok(c) => c,
                Err(e) => {
                    yield Err(CompletionError::ProviderError(format!("Stream error: {e}")));
                    return;
                }
            };
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            // Process complete SSE events (double newline delimited).
            while let Some(boundary) = buffer.find("\n\n") {
                let event_block = buffer[..boundary].to_string();
                buffer = buffer[boundary + 2..].to_string();

                for line in event_block.lines() {
                    let data = if let Some(d) = line.strip_prefix("data: ") {
                        d.trim()
                    } else if let Some(d) = line.strip_prefix("data:") {
                        d.trim()
                    } else {
                        continue;
                    };

                    if data == "[DONE]" {
                        // Yield final response.
                        let output_len = accumulated_text.len() as u64;
                        yield Ok(RawStreamingChoice::FinalResponse(CodexStreamingResponse {
                            input_tokens: 0,
                            output_tokens: output_len,
                        }));
                        return;
                    }

                    let Ok(event) = serde_json::from_str::<serde_json::Value>(data) else {
                        continue;
                    };

                    let event_type = event.get("type").and_then(serde_json::Value::as_str).unwrap_or("");

                    match event_type {
                        "response.output_text.delta" => {
                            if let Some(delta) = event.get("delta").and_then(serde_json::Value::as_str) {
                                accumulated_text.push_str(delta);
                                yield Ok(RawStreamingChoice::Message(delta.to_string()));
                            }
                        }
                        "response.output_text.done" => {
                            if let Some(text) = event.get("text").and_then(serde_json::Value::as_str) {
                                done_text = Some(text.to_string());
                            }
                        }
                        "response.output_item.added" => {
                            // Captures name and call_id for function_call items.
                            if event.get("item").and_then(|i| i.get("type")).and_then(serde_json::Value::as_str) == Some("function_call") {
                                let idx = event.get("output_index").and_then(serde_json::Value::as_u64).unwrap_or(0);
                                let item = &event["item"];
                                let name = item.get("name").and_then(serde_json::Value::as_str).unwrap_or("unknown").to_string();
                                let call_id = item.get("call_id").and_then(serde_json::Value::as_str).unwrap_or("").to_string();
                                fn_call_meta.insert(idx, (name, call_id));
                            }
                        }
                        "response.function_call_arguments.done" => {
                            let output_index = event.get("output_index").and_then(serde_json::Value::as_u64).unwrap_or(0);

                            // Prefer metadata from output_item.added; fall back to fields on this event.
                            let (name, call_id) = fn_call_meta.remove(&output_index).unwrap_or_else(|| {
                                let name = event.get("name")
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or("unknown")
                                    .to_string();
                                let call_id = event.get("call_id")
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or("")
                                    .to_string();
                                (name, call_id)
                            });

                            // Codex API rejects empty call_id — generate one if missing.
                            let call_id = if call_id.is_empty() {
                                format!("call_{}", uuid::Uuid::new_v4().simple())
                            } else {
                                call_id
                            };

                            let arguments_str = event.get("arguments")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("{}");
                            let arguments: serde_json::Value = serde_json::from_str(arguments_str)
                                .unwrap_or(serde_json::json!({}));

                            let internal_call_id = format!("codex-{}", uuid::Uuid::new_v4());

                            // Cache for reconstructing missing function_call items on next turn.
                            if let Ok(mut guard) = tool_call_cache.lock() {
                                guard.insert(call_id.clone(), (name.clone(), arguments_str.to_string()));
                            }

                            yield Ok(RawStreamingChoice::ToolCall(RawStreamingToolCall {
                                id: call_id.clone(),
                                internal_call_id,
                                call_id: Some(call_id),
                                name,
                                arguments,
                                signature: None,
                                additional_params: None,
                            }));
                        }
                        "error" | "response.failed" => {
                            let error_msg = extract_error_message(&event);
                            yield Err(CompletionError::ProviderError(error_msg));
                            return;
                        }
                        "response.completed" | "response.done" => {
                            // Extract usage if present.
                            let usage = event.pointer("/response/usage");
                            let input_tokens = usage
                                .and_then(|u| u.get("input_tokens"))
                                .and_then(serde_json::Value::as_u64)
                                .unwrap_or(0);
                            let output_tokens = usage
                                .and_then(|u| u.get("output_tokens"))
                                .and_then(serde_json::Value::as_u64)
                                .unwrap_or(accumulated_text.len() as u64);

                            // If we got no deltas, try to extract text from the response.
                            if accumulated_text.is_empty()
                                && let Some(text) = extract_text_from_response(&event)
                            {
                                yield Ok(RawStreamingChoice::Message(text));
                            }

                            yield Ok(RawStreamingChoice::FinalResponse(CodexStreamingResponse {
                                input_tokens,
                                output_tokens,
                            }));
                            return;
                        }
                        _ => {
                            // response.created, response.in_progress, etc. -- ignored.
                        }
                    }
                }
            }
        }

        // Stream ended without [DONE] — yield what we have.
        if accumulated_text.is_empty()
            && let Some(text) = done_text
        {
            yield Ok(RawStreamingChoice::Message(text));
        }
        let output_len = accumulated_text.len() as u64;
        yield Ok(RawStreamingChoice::FinalResponse(CodexStreamingResponse {
            input_tokens: 0,
            output_tokens: output_len,
        }));
    })
}

fn extract_error_message(event: &serde_json::Value) -> String {
    event
        .get("message")
        .and_then(serde_json::Value::as_str)
        .or_else(|| event.get("code").and_then(serde_json::Value::as_str))
        .or_else(|| {
            event
                .pointer("/error/message")
                .and_then(serde_json::Value::as_str)
        })
        .or_else(|| {
            event
                .pointer("/response/error/message")
                .and_then(serde_json::Value::as_str)
        })
        .unwrap_or("Unknown Codex API error")
        .to_string()
}

fn extract_text_from_response(event: &serde_json::Value) -> Option<String> {
    if let Some(text) = event
        .pointer("/response/output_text")
        .and_then(serde_json::Value::as_str)
        && !text.is_empty()
    {
        return Some(text.to_string());
    }

    let outputs = event
        .pointer("/response/output")
        .and_then(serde_json::Value::as_array)?;
    for output in outputs {
        let Some(content) = output.get("content").and_then(serde_json::Value::as_array) else {
            continue;
        };
        for item in content {
            if item.get("type").and_then(serde_json::Value::as_str) == Some("output_text")
                && let Some(text) = item.get("text").and_then(serde_json::Value::as_str)
                && !text.is_empty()
            {
                return Some(text.to_string());
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_model_strips_prefix() {
        assert_eq!(normalize_model_id("openai/gpt-5-codex"), "gpt-5-codex");
        assert_eq!(normalize_model_id("gpt-5-codex"), "gpt-5-codex");
        assert_eq!(normalize_model_id("vendor/sub/model-name"), "model-name");
    }

    fn user_msg(text: &str) -> Message {
        Message::User {
            content: rig::one_or_many::OneOrMany::one(UserContent::Text(Text {
                text: text.to_string(),
            })),
        }
    }

    #[test]
    fn build_request_body_basic() {
        let request = CompletionRequest {
            model: None,
            preamble: Some("You are a helper.".to_string()),
            chat_history: rig::one_or_many::OneOrMany::one(user_msg("Hello")),
            documents: vec![],
            tools: vec![],
            temperature: None,
            max_tokens: None,
            tool_choice: None,
            output_schema: None,
            additional_params: None,
        };
        let cache: ToolCallCache =
            std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        let body = build_request_body("gpt-5-codex", &request, &cache);
        assert_eq!(body["model"], "gpt-5-codex");
        assert_eq!(body["instructions"], "You are a helper.");
        assert!(body["stream"].as_bool().unwrap());
        let input = body["input"].as_array().unwrap();
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["role"], "user");
    }

    #[test]
    fn build_request_body_with_tools() {
        let request = CompletionRequest {
            model: None,
            preamble: None,
            chat_history: rig::one_or_many::OneOrMany::one(user_msg("test")),
            documents: vec![],
            tools: vec![rig::completion::ToolDefinition {
                name: "shell".to_string(),
                description: "Run a command".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            }],
            temperature: None,
            max_tokens: None,
            tool_choice: None,
            output_schema: None,
            additional_params: None,
        };
        let cache: ToolCallCache =
            std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        let body = build_request_body("gpt-5-codex", &request, &cache);
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "shell");
        assert_eq!(body["tool_choice"], "auto");
    }

    #[test]
    fn build_request_body_system_messages_go_to_instructions() {
        let request = CompletionRequest {
            model: None,
            preamble: Some("Preamble".to_string()),
            chat_history: rig::one_or_many::OneOrMany::many(vec![
                Message::System {
                    content: "Extra system context".to_string(),
                },
                user_msg("Hi"),
            ])
            .unwrap(),
            documents: vec![],
            tools: vec![],
            temperature: None,
            max_tokens: None,
            tool_choice: None,
            output_schema: None,
            additional_params: None,
        };
        let cache: ToolCallCache =
            std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        let body = build_request_body("test-model", &request, &cache);
        let instructions = body["instructions"].as_str().unwrap();
        assert!(instructions.contains("Preamble"));
        assert!(instructions.contains("Extra system context"));
        let input = body["input"].as_array().unwrap();
        assert_eq!(input.len(), 1);
    }

    #[test]
    fn extract_error_message_variants() {
        let e1 = serde_json::json!({"type": "error", "message": "Rate limit"});
        assert_eq!(extract_error_message(&e1), "Rate limit");

        let e2 = serde_json::json!({"type": "error", "code": "overloaded"});
        assert_eq!(extract_error_message(&e2), "overloaded");

        let e3 = serde_json::json!({"type": "error", "error": {"message": "Nested"}});
        assert_eq!(extract_error_message(&e3), "Nested");

        let e4 = serde_json::json!({"type": "response.failed", "response": {"error": {"message": "Deep"}}});
        assert_eq!(extract_error_message(&e4), "Deep");
    }

    #[test]
    fn extract_text_from_output_text() {
        let event = serde_json::json!({
            "response": {"output_text": "Hello world"}
        });
        assert_eq!(
            extract_text_from_response(&event),
            Some("Hello world".to_string())
        );
    }

    #[test]
    fn extract_text_from_output_array() {
        let event = serde_json::json!({
            "response": {
                "output": [{
                    "content": [{"type": "output_text", "text": "From array"}]
                }]
            }
        });
        assert_eq!(
            extract_text_from_response(&event),
            Some("From array".to_string())
        );
    }
}
