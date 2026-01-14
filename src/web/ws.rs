//! WebSocket handler and file watcher for real-time updates.

use anyhow::Result;
use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher, EventKind};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock, mpsc};
use tracing::{info, warn, debug};

use super::types::*;
use super::WebState;
use crate::config::{ARB_THRESHOLD, ENABLED_LEAGUES};
use crate::position_tracker::PositionTracker;

// Track server start time for uptime calculation
static START_TIME: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

fn get_start_time() -> &'static std::time::Instant {
    START_TIME.get_or_init(std::time::Instant::now)
}

/// WebSocket update message types (server -> client)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum WebSocketUpdate {
    /// Position data updated
    Positions(PositionsListResponse),
    /// Summary stats updated
    Summary(SummaryResponse),
    /// Market prices updated
    MarketUpdate {
        market_id: u16,
        kalshi_yes: u16,
        kalshi_no: u16,
        poly_yes: u16,
        poly_no: u16,
    },
    /// Arbitrage opportunity detected
    ArbOpportunity {
        market_id: u16,
        description: String,
        arb_type: String,
        profit_cents: i16,
    },
    /// System heartbeat
    Heartbeat {
        timestamp: String,
        market_count: usize,
    },
    /// Response to ping
    Pong {
        timestamp: String,
    },
    /// Health data
    Health(HealthResponse),
    /// Config data
    Config(ConfigResponse),
    /// Circuit breaker data
    CircuitBreaker(CircuitBreakerResponse),
    /// Markets list
    Markets(MarketsListResponse),
}

/// WebSocket request message types (client -> server)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WebSocketRequest {
    /// Ping request
    Ping,
    /// Request health data
    GetHealth,
    /// Request positions
    GetPositions,
    /// Request summary
    GetSummary,
    /// Request markets
    GetMarkets,
    /// Request config
    GetConfig,
    /// Request circuit breaker status
    GetCircuitBreaker,
}

/// WebSocket upgrade handler
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<WebState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// Handle an individual WebSocket connection
async fn handle_socket(socket: WebSocket, state: WebState) {
    let (mut sender, mut receiver) = socket.split();
    
    // Create channel for sending responses from request handler
    let (response_tx, mut response_rx) = mpsc::channel::<WebSocketUpdate>(32);
    
    // Subscribe to broadcast updates
    let mut update_rx = state.update_tx.subscribe();
    
    // Send initial state
    {
        let tracker = state.position_tracker.read().await;
        let positions: Vec<PositionResponse> = tracker
            .open_positions()
            .iter()
            .map(|p| (*p).into())
            .collect();
        
        let total = positions.len();
        let initial = WebSocketUpdate::Positions(PositionsListResponse { positions, total });
        
        if let Ok(json) = serde_json::to_string(&initial) {
            let _ = sender.send(Message::Text(json)).await;
        }
        
        // Send summary
        let summary = tracker.summary();
        let summary_update = WebSocketUpdate::Summary(SummaryResponse {
            total_cost_basis: summary.total_cost_basis,
            total_guaranteed_profit: summary.total_guaranteed_profit,
            total_unmatched_exposure: summary.total_unmatched_exposure,
            realized_pnl: summary.realized_pnl,
            open_positions: summary.open_positions,
            resolved_positions: summary.resolved_positions,
            total_contracts: summary.total_contracts,
            daily_pnl: tracker.daily_pnl(),
            all_time_pnl: tracker.all_time_pnl,
        });
        
        if let Ok(json) = serde_json::to_string(&summary_update) {
            let _ = sender.send(Message::Text(json)).await;
        }
        
        // Send initial health
        let health = build_health_response(&state);
        if let Ok(json) = serde_json::to_string(&WebSocketUpdate::Health(health)) {
            let _ = sender.send(Message::Text(json)).await;
        }
    }
    
    // Heartbeat task - sends updates every second
    let heartbeat_state = state.clone();
    let heartbeat_tracker = state.position_tracker.clone();
    let heartbeat_tx = state.update_tx.clone();
    let heartbeat_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(1000));
        loop {
            interval.tick().await;
            
            // Send heartbeat with timestamp
            let update = WebSocketUpdate::Heartbeat {
                timestamp: chrono::Utc::now().to_rfc3339(),
                market_count: heartbeat_state.global_state.market_count(),
            };
            let _ = heartbeat_tx.send(update);
            
            // Send current positions
            let tracker = heartbeat_tracker.read().await;
            let positions: Vec<PositionResponse> = tracker
                .open_positions()
                .iter()
                .map(|p| (*p).into())
                .collect();
            let total = positions.len();
            let _ = heartbeat_tx.send(WebSocketUpdate::Positions(PositionsListResponse { positions, total }));
            
            // Send summary
            let summary = tracker.summary();
            let _ = heartbeat_tx.send(WebSocketUpdate::Summary(SummaryResponse {
                total_cost_basis: summary.total_cost_basis,
                total_guaranteed_profit: summary.total_guaranteed_profit,
                total_unmatched_exposure: summary.total_unmatched_exposure,
                realized_pnl: summary.realized_pnl,
                open_positions: summary.open_positions,
                resolved_positions: summary.resolved_positions,
                total_contracts: summary.total_contracts,
                daily_pnl: tracker.daily_pnl(),
                all_time_pnl: tracker.all_time_pnl,
            }));
            
            // Send health update for uptime
            let health = build_health_response(&heartbeat_state);
            let _ = heartbeat_tx.send(WebSocketUpdate::Health(health));
        }
    });
    
    // Sender task - forwards broadcast updates and direct responses to client
    let send_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                // Handle broadcast updates
                Ok(update) = update_rx.recv() => {
                    if let Ok(json) = serde_json::to_string(&update) {
                        if sender.send(Message::Text(json)).await.is_err() {
                            break;
                        }
                    }
                }
                // Handle direct responses
                Some(update) = response_rx.recv() => {
                    if let Ok(json) = serde_json::to_string(&update) {
                        if sender.send(Message::Text(json)).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }
    });
    
    // Handle incoming messages and process requests
    let recv_state = state.clone();
    let recv_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Close(_)) => break,
                Ok(Message::Ping(_)) => {
                    // Pong is handled automatically by axum
                }
                Ok(Message::Text(text)) => {
                    debug!("[WS] Received: {}", text);
                    
                    // Try to parse as a request
                    if let Ok(request) = serde_json::from_str::<WebSocketRequest>(&text) {
                        if let Some(response) = handle_request(request, &recv_state).await {
                            let _ = response_tx.send(response).await;
                        }
                    }
                }
                Err(e) => {
                    warn!("[WS] Error receiving message: {}", e);
                    break;
                }
                _ => {}
            }
        }
    });
    
    // Wait for either task to complete
    tokio::select! {
        _ = send_task => {}
        _ = recv_task => {}
    }
    
    heartbeat_handle.abort();
    info!("[WS] Client disconnected");
}

/// Handle incoming WebSocket requests
async fn handle_request(request: WebSocketRequest, state: &WebState) -> Option<WebSocketUpdate> {
    match request {
        WebSocketRequest::Ping => {
            Some(WebSocketUpdate::Pong {
                timestamp: chrono::Utc::now().to_rfc3339(),
            })
        }
        WebSocketRequest::GetHealth => {
            Some(WebSocketUpdate::Health(build_health_response(state)))
        }
        WebSocketRequest::GetPositions => {
            let tracker = state.position_tracker.read().await;
            let positions: Vec<PositionResponse> = tracker
                .open_positions()
                .iter()
                .map(|p| (*p).into())
                .collect();
            let total = positions.len();
            Some(WebSocketUpdate::Positions(PositionsListResponse { positions, total }))
        }
        WebSocketRequest::GetSummary => {
            let tracker = state.position_tracker.read().await;
            let summary = tracker.summary();
            Some(WebSocketUpdate::Summary(SummaryResponse {
                total_cost_basis: summary.total_cost_basis,
                total_guaranteed_profit: summary.total_guaranteed_profit,
                total_unmatched_exposure: summary.total_unmatched_exposure,
                realized_pnl: summary.realized_pnl,
                open_positions: summary.open_positions,
                resolved_positions: summary.resolved_positions,
                total_contracts: summary.total_contracts,
                daily_pnl: tracker.daily_pnl(),
                all_time_pnl: tracker.all_time_pnl,
            }))
        }
        WebSocketRequest::GetConfig => {
            let dry_run = std::env::var("DRY_RUN")
                .map(|v| v == "1" || v == "true")
                .unwrap_or(true);
            
            let web_port = std::env::var("WEB_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(8080u16);
            
            let cred_status = &state.credential_status;
            
            let config = ConfigResponse {
                dry_run,
                arb_threshold: ARB_THRESHOLD,
                enabled_leagues: ENABLED_LEAGUES.iter().map(|s| s.to_string()).collect(),
                market_count: state.global_state.market_count(),
                web_port,
                credentials: CredentialStatusResponse {
                    kalshi_configured: cred_status.kalshi_configured,
                    kalshi_error: cred_status.kalshi_error.clone(),
                    polymarket_configured: cred_status.polymarket_configured,
                    polymarket_error: cred_status.polymarket_error.clone(),
                },
            };
            Some(WebSocketUpdate::Config(config))
        }
        WebSocketRequest::GetCircuitBreaker => {
            let status = state.circuit_breaker.status().await;
            let cb_config = state.circuit_breaker.config();
            Some(WebSocketUpdate::CircuitBreaker(CircuitBreakerResponse {
                is_trading_allowed: !status.halted,
                is_tripped: status.halted,
                trip_reason: status.trip_reason.map(|r| r.to_string()),
                daily_pnl_cents: (status.daily_pnl * 100.0) as i64,
                total_contracts: status.total_position,
                consecutive_errors: status.consecutive_errors,
                config: CircuitBreakerConfigResponse {
                    enabled: cb_config.enabled,
                    max_position_per_market: cb_config.max_position_per_market,
                    max_total_position: cb_config.max_total_position,
                    max_daily_loss_cents: (cb_config.max_daily_loss * 100.0) as i64,
                    max_consecutive_errors: cb_config.max_consecutive_errors,
                    cooldown_secs: cb_config.cooldown_secs,
                },
            }))
        }
        WebSocketRequest::GetMarkets => {
            // Markets are not stored in WebState, return empty for now
            // The frontend will fetch via REST API
            None
        }
    }
}

/// Build health response from state
fn build_health_response(_state: &WebState) -> HealthResponse {
    let uptime = get_start_time().elapsed().as_secs();
    
    HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_secs: uptime,
        timestamp: chrono::Utc::now().to_rfc3339(),
    }
}

/// Watch positions.json for changes and broadcast updates
pub async fn watch_positions_file(
    tx: broadcast::Sender<WebSocketUpdate>,
    tracker: Arc<RwLock<PositionTracker>>,
) -> Result<()> {
    use tokio::sync::mpsc;
    
    let (notify_tx, mut notify_rx) = mpsc::channel(100);
    
    // Create file watcher
    let mut watcher = RecommendedWatcher::new(
        move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                    let _ = notify_tx.blocking_send(());
                }
            }
        },
        Config::default().with_poll_interval(Duration::from_millis(100)),
    )?;
    
    // Watch positions.json
    let positions_path = Path::new("positions.json");
    if positions_path.exists() {
        watcher.watch(positions_path, RecursiveMode::NonRecursive)?;
        info!("[WEB] Watching positions.json for changes");
    } else {
        info!("[WEB] positions.json not found, will watch when created");
        // Watch current directory for file creation
        watcher.watch(Path::new("."), RecursiveMode::NonRecursive)?;
    }
    
    // Debounce and broadcast updates
    let mut debounce_interval = tokio::time::interval(Duration::from_millis(200));
    let mut pending_update = false;
    
    loop {
        tokio::select! {
            Some(()) = notify_rx.recv() => {
                pending_update = true;
            }
            _ = debounce_interval.tick() => {
                if pending_update {
                    pending_update = false;
                    
                    // Reload positions from file
                    let loaded = PositionTracker::load();
                    
                    // Update shared tracker
                    {
                        let mut guard = tracker.write().await;
                        *guard = loaded;
                    }
                    
                    // Broadcast update
                    let guard = tracker.read().await;
                    let positions: Vec<PositionResponse> = guard
                        .open_positions()
                        .iter()
                        .map(|p| (*p).into())
                        .collect();
                    
                    let total = positions.len();
                    let update = WebSocketUpdate::Positions(PositionsListResponse { positions, total });
                    let _ = tx.send(update);
                    
                    // Also send summary
                    let summary = guard.summary();
                    let summary_update = WebSocketUpdate::Summary(SummaryResponse {
                        total_cost_basis: summary.total_cost_basis,
                        total_guaranteed_profit: summary.total_guaranteed_profit,
                        total_unmatched_exposure: summary.total_unmatched_exposure,
                        realized_pnl: summary.realized_pnl,
                        open_positions: summary.open_positions,
                        resolved_positions: summary.resolved_positions,
                        total_contracts: summary.total_contracts,
                        daily_pnl: guard.daily_pnl(),
                        all_time_pnl: guard.all_time_pnl,
                    });
                    let _ = tx.send(summary_update);
                    
                    info!("[WEB] Broadcasted position update ({} positions)", total);
                }
            }
        }
    }
}
