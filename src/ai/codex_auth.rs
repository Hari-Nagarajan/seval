//! Codex CLI auth token management.
//!
//! Reads tokens from `~/.codex/auth.json` (created by `codex auth login`),
//! auto-refreshes expired access tokens, and writes updated tokens back.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

const OPENAI_OAUTH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const OPENAI_OAUTH_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const REFRESH_SKEW_SECS: i64 = 90;

/// On-disk format of `~/.codex/auth.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexAuthFile {
    #[serde(default)]
    pub auth_mode: Option<String>,
    #[serde(rename = "OPENAI_API_KEY")]
    pub openai_api_key: Option<String>,
    pub tokens: Option<CodexTokens>,
    #[serde(default)]
    pub last_refresh: Option<String>,
}

/// Token set stored inside the auth file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexTokens {
    pub id_token: Option<String>,
    pub access_token: String,
    pub refresh_token: String,
    pub account_id: Option<String>,
}

/// Resolved credentials ready for API calls.
#[derive(Debug, Clone)]
pub struct CodexCredentials {
    pub access_token: String,
    pub account_id: String,
}

/// Thread-safe token manager that handles loading and refreshing.
#[derive(Clone)]
pub struct CodexAuth {
    state: Arc<Mutex<AuthState>>,
    auth_path: PathBuf,
}

impl std::fmt::Debug for CodexAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodexAuth")
            .field("auth_path", &self.auth_path)
            .finish_non_exhaustive()
    }
}

struct AuthState {
    tokens: CodexTokens,
    account_id: String,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl CodexAuth {
    /// Load credentials from `~/.codex/auth.json`.
    pub fn load() -> Result<Self> {
        let home = directories::BaseDirs::new()
            .context("Could not determine home directory")?
            .home_dir()
            .to_path_buf();
        let auth_path = home.join(".codex").join("auth.json");
        Self::load_from(&auth_path)
    }

    /// Load credentials from a specific path.
    pub fn load_from(auth_path: &PathBuf) -> Result<Self> {
        let content = std::fs::read_to_string(auth_path).with_context(|| {
            format!(
                "Could not read {}. Run `codex auth login` first.",
                auth_path.display()
            )
        })?;
        let auth_file: CodexAuthFile =
            serde_json::from_str(&content).context("Failed to parse codex auth.json")?;

        let tokens = auth_file
            .tokens
            .context("No tokens found in codex auth.json. Run `codex auth login`.")?;

        let account_id = tokens
            .account_id
            .clone()
            .or_else(|| extract_account_id_from_jwt(&tokens.access_token))
            .context("Could not determine account_id from codex auth tokens")?;

        let expires_at = extract_exp_from_jwt(&tokens.access_token);

        Ok(Self {
            state: Arc::new(Mutex::new(AuthState {
                tokens,
                account_id,
                expires_at,
            })),
            auth_path: auth_path.clone(),
        })
    }

    /// Get valid credentials, refreshing if needed.
    pub async fn credentials(&self) -> Result<CodexCredentials> {
        let mut state = self.state.lock().await;

        if Self::needs_refresh(&state) {
            self.refresh_tokens(&mut state).await?;
        }

        Ok(CodexCredentials {
            access_token: state.tokens.access_token.clone(),
            account_id: state.account_id.clone(),
        })
    }

    fn needs_refresh(state: &AuthState) -> bool {
        let Some(exp) = state.expires_at else {
            return false;
        };
        let now = chrono::Utc::now();
        exp - now < chrono::Duration::seconds(REFRESH_SKEW_SECS)
    }

    async fn refresh_tokens(&self, state: &mut AuthState) -> Result<()> {
        tracing::info!("Refreshing Codex access token...");

        let client = reqwest::Client::new();
        let resp = client
            .post(OPENAI_OAUTH_TOKEN_URL)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", &state.tokens.refresh_token),
                ("client_id", OPENAI_OAUTH_CLIENT_ID),
            ])
            .send()
            .await
            .context("Token refresh request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("Token refresh failed ({status}): {body}");
        }

        let refresh_resp: RefreshResponse = resp
            .json()
            .await
            .context("Failed to parse refresh response")?;

        state
            .tokens
            .access_token
            .clone_from(&refresh_resp.access_token);
        if let Some(rt) = refresh_resp.refresh_token {
            state.tokens.refresh_token = rt;
        }
        if let Some(id) = refresh_resp.id_token {
            state.tokens.id_token = Some(id);
        }

        if let Some(new_id) = extract_account_id_from_jwt(&state.tokens.access_token) {
            state.account_id = new_id;
        }
        state.expires_at = extract_exp_from_jwt(&state.tokens.access_token);

        state.tokens.account_id = Some(state.account_id.clone());

        // Write updated tokens back to disk.
        if let Err(e) = self.save_tokens(state) {
            tracing::warn!("Failed to write refreshed tokens to disk: {e}");
        }

        tracing::info!("Token refreshed successfully");
        Ok(())
    }

    fn save_tokens(&self, state: &AuthState) -> Result<()> {
        let auth_file = CodexAuthFile {
            auth_mode: Some("chatgpt".to_string()),
            openai_api_key: None,
            tokens: Some(state.tokens.clone()),
            last_refresh: Some(chrono::Utc::now().to_rfc3339()),
        };
        let json = serde_json::to_string_pretty(&auth_file)?;
        std::fs::write(&self.auth_path, json)?;
        Ok(())
    }
}

#[derive(Deserialize)]
#[allow(clippy::struct_field_names)]
struct RefreshResponse {
    access_token: String,
    refresh_token: Option<String>,
    id_token: Option<String>,
}

/// Extract `account_id` from a JWT access token's claims.
fn extract_account_id_from_jwt(token: &str) -> Option<String> {
    let claims = decode_jwt_claims(token)?;

    // Check nested claim first (OpenAI's format).
    if let Some(auth) = claims.get("https://api.openai.com/auth")
        && let Some(id) = auth
            .get("chatgpt_account_id")
            .and_then(serde_json::Value::as_str)
    {
        return Some(id.to_string());
    }

    // Fallback keys.
    for key in ["account_id", "accountId", "acct", "sub"] {
        if let Some(id) = claims.get(key).and_then(serde_json::Value::as_str) {
            return Some(id.to_string());
        }
    }

    None
}

/// Extract `exp` (expiry) timestamp from a JWT.
fn extract_exp_from_jwt(token: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let claims = decode_jwt_claims(token)?;
    let exp = claims.get("exp")?.as_i64()?;
    chrono::DateTime::from_timestamp(exp, 0)
}

/// Decode the claims (middle segment) of a JWT without signature verification.
fn decode_jwt_claims(token: &str) -> Option<serde_json::Value> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let payload = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
    serde_json::from_slice(&payload).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_jwt(claims: &serde_json::Value) -> String {
        let header = URL_SAFE_NO_PAD.encode(b"{}");
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).unwrap());
        format!("{header}.{payload}.fake_signature")
    }

    #[test]
    fn extract_account_id_nested_claim() {
        let claims = serde_json::json!({
            "https://api.openai.com/auth": {
                "chatgpt_account_id": "abc-123"
            }
        });
        let token = make_jwt(&claims);
        assert_eq!(
            extract_account_id_from_jwt(&token),
            Some("abc-123".to_string())
        );
    }

    #[test]
    fn extract_account_id_fallback_keys() {
        for key in ["account_id", "accountId", "acct", "sub"] {
            let claims = serde_json::json!({ key: "test-id" });
            let token = make_jwt(&claims);
            assert_eq!(
                extract_account_id_from_jwt(&token),
                Some("test-id".to_string()),
                "failed for key: {key}"
            );
        }
    }

    #[test]
    fn extract_account_id_missing_returns_none() {
        let claims = serde_json::json!({"foo": "bar"});
        let token = make_jwt(&claims);
        assert_eq!(extract_account_id_from_jwt(&token), None);
    }

    #[test]
    fn extract_exp_valid() {
        let claims = serde_json::json!({"exp": 1_700_000_000});
        let token = make_jwt(&claims);
        let exp = extract_exp_from_jwt(&token).unwrap();
        assert_eq!(exp.timestamp(), 1_700_000_000);
    }

    #[test]
    fn extract_exp_missing_returns_none() {
        let claims = serde_json::json!({"foo": "bar"});
        let token = make_jwt(&claims);
        assert_eq!(extract_exp_from_jwt(&token), None);
    }

    #[test]
    fn decode_jwt_invalid_token() {
        assert_eq!(decode_jwt_claims("not.a.jwt.really"), None);
        assert_eq!(decode_jwt_claims("nope"), None);
    }

    #[test]
    fn parse_codex_auth_file() {
        let json = r#"{
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": null,
            "tokens": {
                "access_token": "at",
                "refresh_token": "rt",
                "account_id": "acc-1"
            }
        }"#;
        let auth: CodexAuthFile = serde_json::from_str(json).unwrap();
        let tokens = auth.tokens.unwrap();
        assert_eq!(tokens.access_token, "at");
        assert_eq!(tokens.refresh_token, "rt");
        assert_eq!(tokens.account_id.unwrap(), "acc-1");
    }

    #[test]
    fn load_from_missing_file() {
        let path = PathBuf::from("/nonexistent/auth.json");
        let err = CodexAuth::load_from(&path).unwrap_err();
        assert!(err.to_string().contains("codex auth login"));
    }
}
