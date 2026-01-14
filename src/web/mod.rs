//! Web server module for the arbitrage dashboard.
//!
//! Provides a REST API and WebSocket interface for real-time monitoring
//! of positions, markets, and system health.

mod api;
mod types;
mod ws;

use anyhow::Result;
use axum::{
    Router,
    routing::get,
};
use std::sync::Arc;
use std::net::SocketAddr;
use tokio::sync::{broadcast, RwLock};
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tracing::{info, error};

use crate::circuit_breaker::CircuitBreaker;
use crate::position_tracker::PositionTracker;
use crate::types::{GlobalState, CredentialStatus};

pub use ws::WebSocketUpdate;

/// Shared state for web handlers
#[derive(Clone)]
pub struct WebState {
    pub global_state: Arc<GlobalState>,
    pub circuit_breaker: Arc<CircuitBreaker>,
    pub position_tracker: Arc<RwLock<PositionTracker>>,
    pub update_tx: broadcast::Sender<WebSocketUpdate>,
    pub credential_status: Arc<CredentialStatus>,
}

/// Configuration for the web server
pub struct WebConfig {
    pub port: u16,
    pub enable_cors: bool,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            port: std::env::var("WEB_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(8080),
            enable_cors: true,
        }
    }
}

/// Start the web server
pub async fn start_server(
    global_state: Arc<GlobalState>,
    circuit_breaker: Arc<CircuitBreaker>,
    position_tracker: Arc<RwLock<PositionTracker>>,
    credential_status: Arc<CredentialStatus>,
) -> Result<()> {
    let config = WebConfig::default();
    
    // Create broadcast channel for WebSocket updates
    let (update_tx, _) = broadcast::channel::<WebSocketUpdate>(256);
    
    let state = WebState {
        global_state,
        circuit_breaker,
        position_tracker: position_tracker.clone(),
        update_tx: update_tx.clone(),
        credential_status,
    };
    
    // Start file watcher for positions.json
    let watcher_tx = update_tx.clone();
    let watcher_tracker = position_tracker.clone();
    tokio::spawn(async move {
        if let Err(e) = ws::watch_positions_file(watcher_tx, watcher_tracker).await {
            error!("[WEB] File watcher failed: {}", e);
        }
    });
    
    // Build router
    let app = build_router(state, &config);
    
    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    info!("🌐 Web dashboard starting at http://localhost:{}", config.port);
    
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    
    Ok(())
}

/// Build the Axum router with all routes
fn build_router(state: WebState, config: &WebConfig) -> Router {
    let api_routes = Router::new()
        .route("/health", get(api::health))
        .route("/positions", get(api::get_positions))
        .route("/positions/summary", get(api::get_position_summary))
        .route("/markets", get(api::get_markets))
        .route("/markets/:id", get(api::get_market))
        .route("/circuit-breaker", get(api::get_circuit_breaker))
        .route("/config", get(api::get_config))
        .with_state(state.clone());
    
    let ws_routes = Router::new()
        .route("/updates", get(ws::ws_handler))
        .with_state(state.clone());
    
    let mut app = Router::new()
        .nest("/api", api_routes)
        .nest("/ws", ws_routes);
    
    // Serve static files from dashboard/dist if it exists
    let dashboard_path = std::path::Path::new("dashboard/dist");
    if dashboard_path.exists() {
        app = app.fallback_service(ServeDir::new(dashboard_path));
        info!("[WEB] Serving dashboard from dashboard/dist");
    } else {
        info!("[WEB] Dashboard not found at dashboard/dist - API only mode");
    }
    
    // Add CORS middleware if enabled
    if config.enable_cors {
        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);
        app = app.layer(cors);
    }
    
    app
}
