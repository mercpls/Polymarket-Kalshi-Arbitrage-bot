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
use tokio::sync::{broadcast, RwLock};
use tracing::{info, warn};

use super::types::*;
use super::WebState;
use crate::position_tracker::PositionTracker;

/// WebSocket update message types
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
    
    // Subscribe to updates
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
    }
    
    // Heartbeat task
    let heartbeat_state = state.clone();
    let heartbeat_tx = state.update_tx.clone();
    let heartbeat_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            let update = WebSocketUpdate::Heartbeat {
                timestamp: chrono::Utc::now().to_rfc3339(),
                market_count: heartbeat_state.global_state.market_count(),
            };
            let _ = heartbeat_tx.send(update);
        }
    });
    
    // Forward updates to this client
    let send_task = tokio::spawn(async move {
        while let Ok(update) = update_rx.recv().await {
            if let Ok(json) = serde_json::to_string(&update) {
                if sender.send(Message::Text(json)).await.is_err() {
                    break;
                }
            }
        }
    });
    
    // Handle incoming messages (for future commands)
    let recv_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Close(_)) => break,
                Ok(Message::Ping(data)) => {
                    // Pong is handled automatically by axum
                }
                Ok(Message::Text(text)) => {
                    // Could handle commands here in the future
                    info!("[WS] Received text: {}", text);
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
