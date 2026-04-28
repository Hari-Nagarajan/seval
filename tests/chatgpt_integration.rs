//! Integration test for the `ChatGPT` (Codex) provider.
//!
//! Requires `~/.codex/auth.json` from `codex auth login`.
//! Skips gracefully if the auth file is missing.

use futures::StreamExt;
use rig::completion::CompletionModel;
use rig::message::{Message, Text, UserContent};
use seval::ai::codex_auth::CodexAuth;
use seval::ai::codex_model::{CodexClient, CodexCompletionModel};

fn skip_if_no_auth() -> Option<CodexAuth> {
    if let Ok(auth) = CodexAuth::load() {
        Some(auth)
    } else {
        eprintln!("Skipping: no ~/.codex/auth.json (run `codex auth login`)");
        None
    }
}

fn hello_request(text: &str) -> rig::completion::CompletionRequest {
    rig::completion::CompletionRequest {
        model: None,
        preamble: Some("Respond with exactly one word.".to_string()),
        chat_history: rig::one_or_many::OneOrMany::one(Message::User {
            content: rig::one_or_many::OneOrMany::one(UserContent::Text(Text {
                text: text.to_string(),
            })),
        }),
        documents: vec![],
        tools: vec![],
        temperature: None,
        max_tokens: Some(10),
        tool_choice: None,
        output_schema: None,
        additional_params: None,
    }
}

#[tokio::test]
async fn chatgpt_hello_prompt() {
    let Some(auth) = skip_if_no_auth() else {
        return;
    };
    let client = CodexClient::new(auth);
    let model = CodexCompletionModel::new(client, "gpt-5.5");

    let response = model.completion(hello_request("Say hello")).await;
    match response {
        Ok(resp) => {
            let text = &resp.raw_response.text;
            eprintln!("ChatGPT response: {text}");
            assert!(!text.is_empty(), "response should not be empty");
        }
        Err(e) => {
            panic!("ChatGPT completion failed: {e}");
        }
    }
}

#[tokio::test]
async fn chatgpt_streaming_works() {
    let Some(auth) = skip_if_no_auth() else {
        return;
    };
    let client = CodexClient::new(auth);
    let model = CodexCompletionModel::new(client, "gpt-5.5");

    let mut stream = model
        .stream(hello_request("Say hi"))
        .await
        .expect("stream should start");
    let mut got_text = false;
    while let Some(item) = stream.next().await {
        match item {
            Ok(_) => got_text = true,
            Err(e) => panic!("Stream error: {e}"),
        }
    }
    assert!(got_text, "should have received at least one stream item");
}
