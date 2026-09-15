pub mod model;

use std::time::Duration;

use async_trait::async_trait;
use axum::{
    Json,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use reqwest::{Client, Url};

use crate::{
    anthropic::{
        json_error,
        schema::{CountTokensResponse, MessagesRequest},
    },
    provider::{CliHandlers, Provider, ProviderRequest, RequestContext, RequestEndpoint},
    providers::{
        anthropic::{forwarded_headers, prepare_body_preserving_unsigned_thinking, relay_response},
        kimi::count_tokens,
    },
};

enum BaseUrl {
    Ready(Url),
    Invalid(String),
}

pub struct DeepSeekProvider {
    client: Client,
    base_url: BaseUrl,
    api_key: Option<String>,
    models: Vec<String>,
}

impl DeepSeekProvider {
    pub fn new() -> Self {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(300))
            .build()
            .expect("DeepSeek HTTP client");
        let base_url = match Url::parse(&crate::config::deepseek_base_url()) {
            Ok(url) => BaseUrl::Ready(url),
            Err(error) => BaseUrl::Invalid(error.to_string()),
        };
        Self {
            client,
            base_url,
            api_key: crate::config::deepseek_api_key(),
            models: crate::config::deepseek_models(),
        }
    }

    #[cfg(test)]
    fn with_client(client: Client, base_url: Url, api_key: Option<String>) -> Self {
        Self {
            client,
            base_url: BaseUrl::Ready(base_url),
            api_key,
            models: vec!["deepseek-flash".to_string(), "deepseek-v4-pro".to_string()],
        }
    }

    async fn relay(&self, request: ProviderRequest, ctx: RequestContext) -> Response {
        let Some(requested_model) = request.body.model.as_deref() else {
            return json_error(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                "Missing model",
            );
        };
        let Some(model) = model::resolve(requested_model, &self.models) else {
            return json_error(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                format!("Unsupported DeepSeek model: {requested_model}"),
            );
        };
        let Some(api_key) = self.api_key.as_deref().filter(|key| !key.is_empty()) else {
            return json_error(
                StatusCode::UNAUTHORIZED,
                "authentication_error",
                "DeepSeek API key is not configured; set CCP_DEEPSEEK_API_KEY, DEEPSEEK_API_KEY, or deepseek.apiKey in config.json",
            );
        };
        let api_key = match HeaderValue::from_str(api_key) {
            Ok(value) => value,
            Err(_) => {
                return json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "api_error",
                    "DeepSeek API key contains invalid header characters",
                );
            }
        };
        let base_url = match &self.base_url {
            BaseUrl::Ready(url) => url,
            BaseUrl::Invalid(error) => {
                return json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "api_error",
                    format!("Invalid DeepSeek base URL: {error}"),
                );
            }
        };

        let (parts, raw) = request.original.into_parts();
        let body = match prepare_body_preserving_unsigned_thinking(raw, model) {
            Ok(body) => body,
            Err(error) => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request_error",
                    error.to_string(),
                );
            }
        };
        let url = endpoint_url(base_url, parts.uri.path(), parts.uri.query());
        let mut headers = forwarded_headers(&parts.headers);
        headers.remove(header::AUTHORIZATION);
        headers.remove("x-api-key");
        headers.insert("x-api-key", api_key);
        headers.insert(
            header::ACCEPT_ENCODING,
            HeaderValue::from_static("identity"),
        );
        if let Some(monitor) = &ctx.monitor {
            monitor.model_resolved(&ctx.req_id, model);
            monitor.upstream_started(&ctx.req_id);
        }
        match self
            .client
            .post(url)
            .headers(headers)
            .body(body)
            .send()
            .await
        {
            Ok(upstream) => relay_response(upstream, ctx),
            Err(error) => json_error(
                StatusCode::BAD_GATEWAY,
                "api_error",
                format!("DeepSeek request failed: {}", error.without_url()),
            ),
        }
    }
}

impl Default for DeepSeekProvider {
    fn default() -> Self {
        Self::new()
    }
}

fn endpoint_url(base_url: &Url, path: &str, query: Option<&str>) -> Url {
    let mut url = base_url.clone();
    let base_path = url.path().trim_end_matches('/');
    let path = path.trim_start_matches('/');
    url.set_path(&format!("{base_path}/{path}"));
    url.set_query(query);
    url
}

#[async_trait]
impl Provider for DeepSeekProvider {
    fn name(&self) -> &'static str {
        "deepseek"
    }

    fn supported_models(&self) -> Vec<String> {
        model::advertised_models_for(&self.models)
    }

    fn cli(&self) -> &'static dyn CliHandlers {
        &DEEPSEEK_CLI
    }

    async fn handle_request(&self, request: ProviderRequest, ctx: RequestContext) -> Response {
        if matches!(&request.endpoint, RequestEndpoint::CountTokens) {
            self.handle_count_tokens(request.body, ctx).await
        } else {
            self.relay(request, ctx).await
        }
    }

    async fn handle_messages(&self, _body: MessagesRequest, _ctx: RequestContext) -> Response {
        json_error(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "DeepSeek passthrough requires the original Messages HTTP request",
        )
    }

    async fn handle_count_tokens(&self, body: MessagesRequest, ctx: RequestContext) -> Response {
        let requested = body.model.as_deref().unwrap_or_default();
        let Some(model) = model::resolve(requested, &self.models) else {
            return json_error(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                format!("Unsupported DeepSeek model: {requested}"),
            );
        };
        let tokens = count_tokens::count_tokens(&body);
        if let Some(monitor) = ctx.monitor.as_ref() {
            monitor.model_resolved(&ctx.req_id, model);
            monitor.usage_updated(&ctx.req_id, Some(tokens), None);
        }
        (
            StatusCode::OK,
            Json(CountTokensResponse {
                input_tokens: tokens,
            }),
        )
            .into_response()
    }
}

struct DeepSeekCli;
static DEEPSEEK_CLI: DeepSeekCli = DeepSeekCli;

impl CliHandlers for DeepSeekCli {
    fn login(&self) -> anyhow::Result<()> {
        anyhow::bail!(
            "DeepSeek uses an API key; set CCP_DEEPSEEK_API_KEY, DEEPSEEK_API_KEY, or deepseek.apiKey in config.json"
        )
    }

    fn device(&self) -> anyhow::Result<()> {
        self.login()
    }

    fn status(&self) -> anyhow::Result<()> {
        let Some(source) = crate::config::deepseek_api_key_source() else {
            anyhow::bail!("Not authenticated");
        };
        println!("API key configured: true");
        println!("Source: {source}");
        println!("Base URL: {}", crate::config::deepseek_base_url());
        Ok(())
    }

    fn logout(&self) -> anyhow::Result<()> {
        anyhow::bail!(
            "DeepSeek credentials are managed through environment variables or config.json"
        )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::{
        Router,
        body::Bytes,
        extract::{OriginalUri, State},
        http::{HeaderMap, Request},
        routing::post,
    };
    use serde_json::{Value, json};

    use super::*;

    #[derive(Debug)]
    struct SeenRequest {
        path_and_query: String,
        authorization: String,
        api_key: String,
        body: Value,
    }

    type Seen = Arc<Mutex<Vec<SeenRequest>>>;

    fn context() -> RequestContext {
        RequestContext {
            req_id: "req_deepseek".to_string(),
            provider: "deepseek".to_string(),
            session_id: None,
            session_seq: None,
            monitor: None,
            traffic: None,
        }
    }

    fn provider_request(body: Value, endpoint: RequestEndpoint) -> ProviderRequest {
        let parsed = serde_json::from_value(body.clone()).unwrap();
        let original = Request::builder()
            .method("POST")
            .uri("/v1/messages?beta=1")
            .header(header::AUTHORIZATION, "Bearer client-token")
            .header("x-api-key", "client-key")
            .header(header::CONTENT_TYPE, "application/json")
            .header("anthropic-version", "2023-06-01")
            .body(Bytes::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        ProviderRequest {
            body: parsed,
            original,
            endpoint,
        }
    }

    async fn capture_request(
        State(seen): State<Seen>,
        OriginalUri(uri): OriginalUri,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Response {
        seen.lock().unwrap().push(SeenRequest {
            path_and_query: uri
                .path_and_query()
                .map(ToString::to_string)
                .unwrap_or_default(),
            authorization: headers
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string(),
            api_key: headers
                .get("x-api-key")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string(),
            body: body.clone(),
        });
        if body["stream"] == true {
            return (
                [(header::CONTENT_TYPE, "text/event-stream")],
                concat!(
                    "event: message_start\n",
                    "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_ds\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"deepseek-flash\",\"content\":[],\"stop_reason\":null,\"usage\":{\"input_tokens\":4,\"output_tokens\":0}}}\n\n",
                    "event: content_block_start\n",
                    "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
                    "event: content_block_delta\n",
                    "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n",
                    "event: message_stop\n",
                    "data: {\"type\":\"message_stop\"}\n\n"
                ),
            )
                .into_response();
        }
        Json(json!({
            "id": "msg_ds",
            "type": "message",
            "role": "assistant",
            "model": "deepseek-flash",
            "content": [{"type": "text", "text": "hello"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 4, "output_tokens": 1}
        }))
        .into_response()
    }

    async fn mock_provider() -> (DeepSeekProvider, Seen, tokio::task::JoinHandle<()>) {
        let seen = Seen::default();
        let app = Router::new()
            .route("/anthropic/v1/messages", post(capture_request))
            .with_state(seen.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let provider = DeepSeekProvider::with_client(
            client,
            Url::parse(&format!("http://{address}/anthropic")).unwrap(),
            Some("deepseek-key".to_string()),
        );
        (provider, seen, server)
    }

    #[tokio::test]
    async fn relay_preserves_base_path_replaces_credentials_and_rewrites_history() {
        let (provider, seen, server) = mock_provider().await;
        let body = json!({
            "model": "deepseek/deepseek-flash",
            "max_tokens": 64,
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "native reasoning", "signature": ""},
                    {"type": "thinking", "thinking": "foreign reasoning", "signature": "ccp:kimi:v1:opaque"}
                ]},
                {"role": "user", "content": "hello"}
            ]
        });
        let response = provider
            .handle_request(
                provider_request(body, RequestEndpoint::Messages(None)),
                context(),
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let response_body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let response_body: Value = serde_json::from_slice(&response_body).unwrap();
        assert_eq!(response_body["content"][0]["text"], "hello");
        server.abort();

        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].path_and_query, "/anthropic/v1/messages?beta=1");
        assert!(seen[0].authorization.is_empty());
        assert_eq!(seen[0].api_key, "deepseek-key");
        assert_eq!(seen[0].body["model"], "deepseek-flash");
        assert_eq!(
            seen[0].body["messages"][0]["content"][0]["type"],
            "thinking"
        );
        assert_eq!(seen[0].body["messages"][0]["content"][1]["type"], "text");
        assert!(
            seen[0].body["messages"][0]["content"][1]["text"]
                .as_str()
                .unwrap()
                .contains("foreign reasoning")
        );
    }

    #[tokio::test]
    async fn relays_anthropic_stream() {
        let (provider, seen, server) = mock_provider().await;
        let body = json!({
            "model": "deepseek/deepseek-flash",
            "max_tokens": 64,
            "stream": true,
            "messages": [{"role": "user", "content": "hello"}]
        });
        let response = provider
            .handle_request(
                provider_request(body, RequestEndpoint::Messages(None)),
                context(),
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/event-stream")
        );
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let output = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(output.contains("hello"));
        assert!(output.contains("event: message_stop"));
        assert_eq!(seen.lock().unwrap().len(), 1);
        server.abort();
    }

    #[tokio::test]
    async fn missing_key_is_actionable_and_count_tokens_stays_local() {
        let client = Client::builder().build().unwrap();
        let provider = DeepSeekProvider::with_client(
            client,
            Url::parse("https://example.com/anthropic").unwrap(),
            None,
        );
        let body = json!({
            "model": "deepseek/deepseek-flash",
            "max_tokens": 64,
            "messages": [{"role": "user", "content": "hello world"}]
        });
        let response = provider
            .handle_request(
                provider_request(body.clone(), RequestEndpoint::Messages(None)),
                context(),
            )
            .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&bytes).contains("DEEPSEEK_API_KEY"));

        let response = provider
            .handle_request(
                provider_request(body, RequestEndpoint::CountTokens),
                context(),
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(value["input_tokens"].as_u64().unwrap() > 0);
    }
}
