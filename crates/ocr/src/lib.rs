use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use url::Url;

pub use nsh_core as core;

const DEFAULT_TOKEN_URL: &str = "https://aip.baidubce.com/oauth/2.0/token";
const DEFAULT_OCR_URL: &str = "https://aip.baidubce.com/rest/2.0/ocr/v1/general_basic";
const TOKEN_EXPIRY_SKEW: Duration = Duration::from_secs(300);

#[derive(Debug, thiserror::Error)]
pub enum OcrError {
    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("invalid endpoint url: {0}")]
    Url(#[from] url::ParseError),
    #[error("baidu ocr api error {code}: {message}")]
    Api { code: i64, message: String },
    #[error("baidu response did not include an access token")]
    MissingAccessToken,
}

pub type Result<T> = std::result::Result<T, OcrError>;

#[derive(Clone, Debug)]
pub struct BaiduOcrConfig {
    pub api_key: String,
    pub secret_key: String,
    pub token_url: Url,
    pub ocr_url: Url,
}

impl BaiduOcrConfig {
    pub fn new(api_key: impl Into<String>, secret_key: impl Into<String>) -> Result<Self> {
        Ok(Self {
            api_key: api_key.into(),
            secret_key: secret_key.into(),
            token_url: Url::parse(DEFAULT_TOKEN_URL)?,
            ocr_url: Url::parse(DEFAULT_OCR_URL)?,
        })
    }

    pub fn with_endpoints(mut self, token_url: Url, ocr_url: Url) -> Self {
        self.token_url = token_url;
        self.ocr_url = ocr_url;
        self
    }
}

#[derive(Clone, Debug)]
pub struct BaiduOcrClient {
    http: Client,
    config: BaiduOcrConfig,
    token: std::sync::Arc<tokio::sync::Mutex<Option<CachedAccessToken>>>,
}

impl BaiduOcrClient {
    pub fn new(api_key: impl Into<String>, secret_key: impl Into<String>) -> Result<Self> {
        Ok(Self::from_config(BaiduOcrConfig::new(api_key, secret_key)?))
    }

    pub fn from_config(config: BaiduOcrConfig) -> Self {
        Self::with_http_client(config, Client::new())
    }

    pub fn with_http_client(config: BaiduOcrConfig, http: Client) -> Self {
        Self {
            http,
            config,
            token: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    pub async fn access_token(&self) -> Result<String> {
        if let Some(token) = self.valid_cached_token().await {
            return Ok(token);
        }

        let mut guard = self.token.lock().await;
        if let Some(token) = guard.as_ref().filter(|token| token.is_valid()).cloned() {
            return Ok(token.access_token);
        }

        let token = self.fetch_access_token().await?;
        let access_token = token.access_token.clone();
        *guard = Some(token);
        Ok(access_token)
    }

    pub async fn refresh_access_token(&self) -> Result<String> {
        let token = self.fetch_access_token().await?;
        let access_token = token.access_token.clone();
        *self.token.lock().await = Some(token);
        Ok(access_token)
    }

    pub async fn recognize_png_bytes(&self, bytes: &[u8]) -> Result<OcrText> {
        self.recognize_base64(&STANDARD.encode(bytes)).await
    }

    pub async fn recognize_base64(&self, image_base64: &str) -> Result<OcrText> {
        let token = self.access_token().await?;
        let url = ocr_url_with_token(self.config.ocr_url.clone(), &token);
        let payload = build_ocr_payload(image_base64);
        let response = self
            .http
            .post(url)
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .form(&payload)
            .send()
            .await?
            .error_for_status()?
            .json::<BaiduOcrResponse>()
            .await?;

        response.into_result()
    }

    async fn valid_cached_token(&self) -> Option<String> {
        self.token
            .lock()
            .await
            .as_ref()
            .filter(|token| token.is_valid())
            .map(|token| token.access_token.clone())
    }

    async fn fetch_access_token(&self) -> Result<CachedAccessToken> {
        let request = BaiduTokenRequest::new(&self.config.api_key, &self.config.secret_key);
        let response = self
            .http
            .post(self.config.token_url.clone())
            .form(&request)
            .send()
            .await?
            .error_for_status()?
            .json::<BaiduTokenResponse>()
            .await?;

        parse_token_response(response)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OcrText {
    pub lines: Vec<String>,
    pub text: String,
}

#[derive(Clone, Debug)]
struct CachedAccessToken {
    access_token: String,
    expires_at: Instant,
}

impl CachedAccessToken {
    fn is_valid(&self) -> bool {
        Instant::now() < self.expires_at
    }
}

#[derive(Debug, Serialize)]
pub struct BaiduTokenRequest<'a> {
    grant_type: &'static str,
    client_id: &'a str,
    client_secret: &'a str,
}

impl<'a> BaiduTokenRequest<'a> {
    pub fn new(client_id: &'a str, client_secret: &'a str) -> Self {
        Self {
            grant_type: "client_credentials",
            client_id,
            client_secret,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct BaiduTokenResponse {
    access_token: Option<String>,
    expires_in: Option<u64>,
    error: Option<String>,
    error_description: Option<String>,
}

fn parse_token_response(response: BaiduTokenResponse) -> Result<CachedAccessToken> {
    if let Some(error) = response.error {
        return Err(OcrError::Api {
            code: 0,
            message: response.error_description.unwrap_or(error),
        });
    }

    let access_token = response.access_token.ok_or(OcrError::MissingAccessToken)?;
    let expires_in = response.expires_in.unwrap_or(2_592_000);
    let usable_for = Duration::from_secs(expires_in).saturating_sub(TOKEN_EXPIRY_SKEW);
    Ok(CachedAccessToken {
        access_token,
        expires_at: Instant::now() + usable_for,
    })
}

#[derive(Debug, Deserialize)]
struct BaiduOcrResponse {
    words_result: Option<Vec<BaiduWordsResult>>,
    error_code: Option<i64>,
    error_msg: Option<String>,
}

impl BaiduOcrResponse {
    fn into_result(self) -> Result<OcrText> {
        if let Some(code) = self.error_code {
            return Err(OcrError::Api {
                code,
                message: self
                    .error_msg
                    .unwrap_or_else(|| "unknown error".to_string()),
            });
        }

        let lines: Vec<String> = self
            .words_result
            .unwrap_or_default()
            .into_iter()
            .map(|item| item.words)
            .collect();
        let text = lines.join("\n");
        Ok(OcrText { lines, text })
    }
}

#[derive(Debug, Deserialize)]
struct BaiduWordsResult {
    words: String,
}

pub fn build_ocr_payload(image_base64: &str) -> Vec<(&'static str, String)> {
    vec![("image", image_base64.to_string())]
}

fn ocr_url_with_token(mut url: Url, token: &str) -> Url {
    url.query_pairs_mut().append_pair("access_token", token);
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_token_response() {
        let token = parse_token_response(BaiduTokenResponse {
            access_token: Some("abc".to_string()),
            expires_in: Some(3600),
            error: None,
            error_description: None,
        })
        .unwrap();

        assert_eq!(token.access_token, "abc");
        assert!(token.is_valid());
    }

    #[test]
    fn builds_ocr_payload() {
        let payload = build_ocr_payload("aGVsbG8=");
        assert_eq!(payload, vec![("image", "aGVsbG8=".to_string())]);
    }

    #[test]
    fn converts_ocr_response_to_text() {
        let response = BaiduOcrResponse {
            words_result: Some(vec![
                BaiduWordsResult {
                    words: "line one".to_string(),
                },
                BaiduWordsResult {
                    words: "line two".to_string(),
                },
            ]),
            error_code: None,
            error_msg: None,
        };

        let text = response.into_result().unwrap();
        assert_eq!(text.lines, vec!["line one", "line two"]);
        assert_eq!(text.text, "line one\nline two");
    }
}
