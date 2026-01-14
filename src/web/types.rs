//! API response types for the web dashboard.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::position_tracker::{ArbPosition, PositionLeg, PositionSummary};
use crate::types::MarketType;

/// Health check response
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub uptime_secs: u64,
    pub timestamp: String,
}

/// Position with computed fields for the API
#[derive(Debug, Serialize)]
pub struct PositionResponse {
    pub market_id: String,
    pub description: String,
    pub kalshi_yes: LegResponse,
    pub kalshi_no: LegResponse,
    pub poly_yes: LegResponse,
    pub poly_no: LegResponse,
    pub total_fees: f64,
    pub total_cost: f64,
    pub guaranteed_profit: f64,
    pub matched_contracts: f64,
    pub unmatched_exposure: f64,
    pub status: String,
    pub realized_pnl: Option<f64>,
    pub opened_at: String,
}

/// Position leg response
#[derive(Debug, Serialize)]
pub struct LegResponse {
    pub contracts: f64,
    pub cost_basis: f64,
    pub avg_price: f64,
}

impl From<&PositionLeg> for LegResponse {
    fn from(leg: &PositionLeg) -> Self {
        Self {
            contracts: leg.contracts,
            cost_basis: leg.cost_basis,
            avg_price: leg.avg_price,
        }
    }
}

impl From<&ArbPosition> for PositionResponse {
    fn from(pos: &ArbPosition) -> Self {
        Self {
            market_id: pos.market_id.clone(),
            description: pos.description.clone(),
            kalshi_yes: (&pos.kalshi_yes).into(),
            kalshi_no: (&pos.kalshi_no).into(),
            poly_yes: (&pos.poly_yes).into(),
            poly_no: (&pos.poly_no).into(),
            total_fees: pos.total_fees,
            total_cost: pos.total_cost(),
            guaranteed_profit: pos.guaranteed_profit(),
            matched_contracts: pos.matched_contracts(),
            unmatched_exposure: pos.unmatched_exposure(),
            status: pos.status.clone(),
            realized_pnl: pos.realized_pnl,
            opened_at: pos.opened_at.clone(),
        }
    }
}

/// Position summary response
#[derive(Debug, Serialize)]
pub struct SummaryResponse {
    pub total_cost_basis: f64,
    pub total_guaranteed_profit: f64,
    pub total_unmatched_exposure: f64,
    pub realized_pnl: f64,
    pub open_positions: usize,
    pub resolved_positions: usize,
    pub total_contracts: f64,
    pub daily_pnl: f64,
    pub all_time_pnl: f64,
}

/// Market data response
#[derive(Debug, Serialize)]
pub struct MarketResponse {
    pub market_id: u16,
    pub pair_id: String,
    pub league: String,
    pub market_type: String,
    pub description: String,
    pub kalshi_ticker: String,
    pub poly_slug: String,
    pub kalshi_yes_price: u16,
    pub kalshi_no_price: u16,
    pub poly_yes_price: u16,
    pub poly_no_price: u16,
    pub best_arb_type: Option<String>,
    pub best_arb_profit_cents: Option<i16>,
}

/// Circuit breaker status response
#[derive(Debug, Serialize)]
pub struct CircuitBreakerResponse {
    pub is_trading_allowed: bool,
    pub is_tripped: bool,
    pub trip_reason: Option<String>,
    pub daily_pnl_cents: i64,
    pub total_contracts: i64,
    pub consecutive_errors: u32,
    pub config: CircuitBreakerConfigResponse,
}

/// Circuit breaker config response
#[derive(Debug, Serialize)]
pub struct CircuitBreakerConfigResponse {
    pub enabled: bool,
    pub max_position_per_market: i64,
    pub max_total_position: i64,
    pub max_daily_loss_cents: i64,
    pub max_consecutive_errors: u32,
    pub cooldown_secs: u64,
}

/// Bot configuration response
#[derive(Debug, Serialize)]
pub struct ConfigResponse {
    pub dry_run: bool,
    pub arb_threshold: f64,
    pub enabled_leagues: Vec<String>,
    pub market_count: usize,
    pub web_port: u16,
}

/// Market list response
#[derive(Debug, Serialize)]
pub struct MarketsListResponse {
    pub markets: Vec<MarketResponse>,
    pub total: usize,
}

/// Positions list response
#[derive(Debug, Serialize)]
pub struct PositionsListResponse {
    pub positions: Vec<PositionResponse>,
    pub total: usize,
}
