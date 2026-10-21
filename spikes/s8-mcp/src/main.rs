//! Phase 0 S8 spike: rmcp streamable HTTP server on loopback, pane identity from a per-pane bearer token.
use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::Response,
};
use rmcp::{
    handler::server::{router::tool::ToolRouter, tool::Extension},
    model::{ServerCapabilities, ServerConfig},
    tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    },
    ServerHandler,
};

#[derive(Clone, Debug)]
struct PaneId(String);

/// token -> pane. Spike only: tokens come from TOK_A / TOK_B.
type Tokens = Arc<Vec<(String, String)>>;

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

async fn auth(State(tokens): State<Tokens>, headers: HeaderMap, mut req: Request, next: Next) -> Result<Response, StatusCode> {
    if headers.contains_key("origin") {
        eprintln!("rejected: Origin present");
        return Err(StatusCode::FORBIDDEN);
    }
    let presented = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let pane = tokens
        .iter()
        .find(|(tok, _)| constant_time_eq(tok.as_bytes(), presented.as_bytes()))
        .map(|(_, pane)| pane.clone())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let proto = headers.get("mcp-protocol-version").and_then(|v| v.to_str().ok()).unwrap_or("-");
    eprintln!("{} {} pane={} protocol={}", req.method(), req.uri().path(), pane, proto);
    req.extensions_mut().insert(PaneId(pane));
    Ok(next.run(req).await)
}

#[derive(Clone)]
struct Spike {
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl Spike {
    fn new() -> Self {
        Self { tool_router: Self::tool_router() }
    }

    #[tool(description = "Return the agentdeck pane id of the caller.")]
    async fn whoami(&self, Extension(parts): Extension<axum::http::request::Parts>) -> String {
        match parts.extensions.get::<PaneId>() {
            Some(PaneId(p)) => format!("you are pane {p}"),
            None => "no pane".to_string(),
        }
    }
}

#[tool_handler]
impl ServerHandler for Spike {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("SPIKE-INSTRUCTIONS-OK. Call whoami to learn your pane id.")
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let tokens: Tokens = Arc::new(vec![
        (std::env::var("TOK_A").unwrap_or_else(|_| "tok-a".into()), "p1".into()),
        (std::env::var("TOK_B").unwrap_or_else(|_| "tok-b".into()), "p2".into()),
    ]);
    let service = StreamableHttpService::new(
        || Ok(Spike::new()),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default(),
    );
    let app = axum::Router::new()
        .nest_service("/mcp", service)
        .layer(middleware::from_fn_with_state(tokens, auth));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    println!("PORT={}", listener.local_addr()?.port());
    axum::serve(listener, app).await
}
