//! REST API handlers for the web dashboard.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use std::time::Instant;
use std::path::Path as FilePath;
use std::io::Write;
use tracing::{info, error};

use super::types::*;
use super::WebState;
use crate::config::{ARB_THRESHOLD, ENABLED_LEAGUES};
use crate::types::{ArbType, kalshi_fee_cents, NO_PRICE};

// Track server start time for uptime calculation
static START_TIME: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

fn get_start_time() -> &'static Instant {
    START_TIME.get_or_init(Instant::now)
}

/// Health check endpoint
pub async fn health() -> Json<HealthResponse> {
    let uptime = get_start_time().elapsed().as_secs();
    
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_secs: uptime,
        timestamp: chrono::Utc::now().to_rfc3339(),
    })
}

/// Get all positions
pub async fn get_positions(
    State(state): State<WebState>,
) -> Json<PositionsListResponse> {
    let tracker = state.position_tracker.read().await;
    let positions: Vec<PositionResponse> = tracker
        .open_positions()
        .iter()
        .map(|p| (*p).into())
        .collect();
    
    let total = positions.len();
    
    Json(PositionsListResponse { positions, total })
}

/// Get position summary
pub async fn get_position_summary(
    State(state): State<WebState>,
) -> Json<SummaryResponse> {
    let tracker = state.position_tracker.read().await;
    let summary = tracker.summary();
    
    Json(SummaryResponse {
        total_cost_basis: summary.total_cost_basis,
        total_guaranteed_profit: summary.total_guaranteed_profit,
        total_unmatched_exposure: summary.total_unmatched_exposure,
        realized_pnl: summary.realized_pnl,
        open_positions: summary.open_positions,
        resolved_positions: summary.resolved_positions,
        total_contracts: summary.total_contracts,
        daily_pnl: tracker.daily_pnl(),
        all_time_pnl: tracker.all_time_pnl,
    })
}

/// Get all markets with orderbook state
pub async fn get_markets(
    State(state): State<WebState>,
) -> Json<MarketsListResponse> {
    let market_count = state.global_state.market_count();
    let mut markets = Vec::with_capacity(market_count);
    
    for i in 0..market_count {
        if let Some(market) = state.global_state.get_by_id(i as u16) {
            if let Some(pair) = &market.pair {
                let (k_yes, k_no, _, _) = market.kalshi.load();
                let (p_yes, p_no, _, _) = market.poly.load();
                
                // Calculate best arb opportunity
                let (best_arb_type, best_arb_profit_cents) = calculate_best_arb(k_yes, k_no, p_yes, p_no);
                
                markets.push(MarketResponse {
                    market_id: i as u16,
                    pair_id: pair.pair_id.to_string(),
                    league: pair.league.to_string(),
                    market_type: pair.market_type.to_string(),
                    description: pair.description.to_string(),
                    kalshi_ticker: pair.kalshi_market_ticker.to_string(),
                    poly_slug: pair.poly_slug.to_string(),
                    kalshi_yes_price: k_yes,
                    kalshi_no_price: k_no,
                    poly_yes_price: p_yes,
                    poly_no_price: p_no,
                    best_arb_type,
                    best_arb_profit_cents,
                });
            }
        }
    }
    
    let total = markets.len();
    Json(MarketsListResponse { markets, total })
}

/// Get single market by ID
pub async fn get_market(
    State(state): State<WebState>,
    Path(id): Path<u16>,
) -> Result<Json<MarketResponse>, StatusCode> {
    let market = state.global_state.get_by_id(id)
        .ok_or(StatusCode::NOT_FOUND)?;
    
    let pair = market.pair.as_ref()
        .ok_or(StatusCode::NOT_FOUND)?;
    
    let (k_yes, k_no, _, _) = market.kalshi.load();
    let (p_yes, p_no, _, _) = market.poly.load();
    
    let (best_arb_type, best_arb_profit_cents) = calculate_best_arb(k_yes, k_no, p_yes, p_no);
    
    Ok(Json(MarketResponse {
        market_id: id,
        pair_id: pair.pair_id.to_string(),
        league: pair.league.to_string(),
        market_type: pair.market_type.to_string(),
        description: pair.description.to_string(),
        kalshi_ticker: pair.kalshi_market_ticker.to_string(),
        poly_slug: pair.poly_slug.to_string(),
        kalshi_yes_price: k_yes,
        kalshi_no_price: k_no,
        poly_yes_price: p_yes,
        poly_no_price: p_no,
        best_arb_type,
        best_arb_profit_cents,
    }))
}

/// Get circuit breaker status
pub async fn get_circuit_breaker(
    State(state): State<WebState>,
) -> Json<CircuitBreakerResponse> {
    let status = state.circuit_breaker.status().await;
    let config = state.circuit_breaker.config();
    
    Json(CircuitBreakerResponse {
        is_trading_allowed: !status.halted,
        is_tripped: status.halted,
        trip_reason: status.trip_reason.map(|r| r.to_string()),
        daily_pnl_cents: (status.daily_pnl * 100.0) as i64,
        total_contracts: status.total_position,
        consecutive_errors: status.consecutive_errors,
        config: CircuitBreakerConfigResponse {
            enabled: config.enabled,
            max_position_per_market: config.max_position_per_market,
            max_total_position: config.max_total_position,
            max_daily_loss_cents: (config.max_daily_loss * 100.0) as i64,
            max_consecutive_errors: config.max_consecutive_errors,
            cooldown_secs: config.cooldown_secs,
        },
    })
}

/// Get bot configuration
pub async fn get_config(
    State(state): State<WebState>,
) -> Json<ConfigResponse> {
    let dry_run = std::env::var("DRY_RUN")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(true);
    
    let web_port = std::env::var("WEB_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8080u16);
    
    let cred_status = &state.credential_status;
    
    Json(ConfigResponse {
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
    })
}

/// Calculate the best arbitrage opportunity for a market
fn calculate_best_arb(
    k_yes: u16,
    k_no: u16,
    p_yes: u16,
    p_no: u16,
) -> (Option<String>, Option<i16>) {
    if k_yes == NO_PRICE || k_no == NO_PRICE || p_yes == NO_PRICE || p_no == NO_PRICE {
        return (None, None);
    }
    
    let k_yes_fee = kalshi_fee_cents(k_yes);
    let k_no_fee = kalshi_fee_cents(k_no);
    
    // Calculate costs for each arb type
    let costs = [
        (ArbType::PolyYesKalshiNo, p_yes + k_no + k_no_fee),
        (ArbType::KalshiYesPolyNo, k_yes + k_yes_fee + p_no),
        (ArbType::PolyOnly, p_yes + p_no),
        (ArbType::KalshiOnly, k_yes + k_yes_fee + k_no + k_no_fee),
    ];
    
    // Find the best (lowest cost)
    let best = costs.iter()
        .min_by_key(|(_, cost)| cost)
        .unwrap();
    
    let profit = 100i16 - best.1 as i16;
    
    if profit > 0 {
        let arb_type_str = match best.0 {
            ArbType::PolyYesKalshiNo => "poly_yes_kalshi_no",
            ArbType::KalshiYesPolyNo => "kalshi_yes_poly_no",
            ArbType::PolyOnly => "poly_only",
            ArbType::KalshiOnly => "kalshi_only",
        };
        (Some(arb_type_str.to_string()), Some(profit))
    } else {
        (None, Some(profit))
    }
}

/// Reset the circuit breaker
pub async fn reset_circuit_breaker(
    State(state): State<WebState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state.circuit_breaker.reset().await;
    info!("[API] Circuit breaker reset via API");
    Ok(Json(serde_json::json!({ "status": "ok", "message": "Circuit breaker reset" })))
}

/// Halt trading via circuit breaker
pub async fn halt_circuit_breaker(
    State(state): State<WebState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state.circuit_breaker.halt().await;
    info!("[API] Trading halted via API");
    Ok(Json(serde_json::json!({ "status": "ok", "message": "Trading halted" })))
}

/// Save bot configuration to INI file
pub async fn save_config(
    Json(config): Json<SaveConfigRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Build INI content
    let ini_content = format!(
r#"# SpreadHunter Trading Configuration
# Auto-generated by dashboard - DO NOT EDIT MANUALLY WHILE BOT IS RUNNING

[trading]
dry_run = {}
arb_threshold = {}
price_logging = {}

[leagues]
# Empty = all leagues enabled
enabled = {}

[circuit_breaker]
enabled = {}
max_position_per_market = {}
max_total_position = {}
max_daily_loss = {}
max_consecutive_errors = {}
cooldown_secs = {}
"#,
        config.dry_run,
        config.arb_threshold,
        config.price_logging,
        config.enabled_leagues.join(","),
        config.circuit_breaker_enabled,
        config.max_position_per_market,
        config.max_total_position,
        config.max_daily_loss as i64,
        config.max_consecutive_errors,
        config.cooldown_secs,
    );

    // Write to config.ini
    let config_path = FilePath::new("config.ini");
    match std::fs::File::create(config_path) {
        Ok(mut file) => {
            if let Err(e) = file.write_all(ini_content.as_bytes()) {
                error!("[API] Failed to write config.ini: {}", e);
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
            info!("[API] Configuration saved to config.ini");
            Ok(Json(serde_json::json!({ 
                "status": "ok", 
                "message": "Configuration saved",
                "path": "config.ini"
            })))
        }
        Err(e) => {
            error!("[API] Failed to create config.ini: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}
