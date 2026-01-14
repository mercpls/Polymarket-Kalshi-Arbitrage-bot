//! Prediction Market Arbitrage Trading System
//!
//! A high-performance, production-ready arbitrage trading system for cross-platform
//! prediction markets. This system monitors price discrepancies between Kalshi and
//! Polymarket, executing risk-free arbitrage opportunities in real-time.
//!
//! ## Strategy
//!
//! The core arbitrage strategy exploits the fundamental property of prediction markets:
//! YES + NO = $1.00 (guaranteed). Arbitrage opportunities exist when:
//!
//! ```
//! Best YES ask (Platform A) + Best NO ask (Platform B) < $1.00
//! ```
//!
//! ## Architecture
//!
//! - **Real-time price monitoring** via WebSocket connections to both platforms
//! - **Lock-free orderbook cache** using atomic operations for zero-copy updates
//! - **SIMD-accelerated arbitrage detection** for sub-millisecond latency
//! - **Concurrent order execution** with automatic position reconciliation
//! - **Circuit breaker protection** with configurable risk limits
//! - **Market discovery system** with intelligent caching and incremental updates

mod cache;
mod circuit_breaker;
mod config;
mod discovery;
mod execution;
mod kalshi;
mod polymarket;
mod polymarket_clob;
mod position_tracker;
mod types;
mod web;

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use cache::TeamCache;
use circuit_breaker::{CircuitBreaker, CircuitBreakerConfig};
use config::{ARB_THRESHOLD, ENABLED_LEAGUES, WS_RECONNECT_DELAY_SECS};
use discovery::DiscoveryClient;
use execution::{ExecutionEngine, create_execution_channel, run_execution_loop};
use kalshi::{KalshiConfig, KalshiApiClient};
use polymarket_clob::{PolymarketAsyncClient, PreparedCreds, SharedAsyncClient};
use position_tracker::{PositionTracker, create_position_channel, position_writer_loop};
use types::{GlobalState, PriceCents, CredentialStatus};

/// Polymarket CLOB API host
const POLY_CLOB_HOST: &str = "https://clob.polymarket.com";
/// Polygon chain ID
const POLYGON_CHAIN_ID: u64 = 137;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("arb_bot=info".parse().unwrap()),
        )
        .init();

    info!("🚀 Prediction Market Arbitrage System v2.0");
    info!("   Profit threshold: <{:.1}¢ ({:.1}% minimum profit)",
          ARB_THRESHOLD * 100.0, (1.0 - ARB_THRESHOLD) * 100.0);
    info!("   Monitored leagues: {:?}", ENABLED_LEAGUES);

    // Check for dry run mode
    let dry_run = std::env::var("DRY_RUN").map(|v| v == "1" || v == "true").unwrap_or(true);
    if dry_run {
        info!("   Mode: DRY RUN (set DRY_RUN=0 to execute)");
    } else {
        warn!("   Mode: LIVE EXECUTION");
    }

    // Track credential status
    let mut cred_status = CredentialStatus::new();
    
    // Try to load Kalshi credentials (non-fatal if missing)
    let (kalshi_config, kalshi_error) = KalshiConfig::try_from_env();
    if let Some(ref config) = kalshi_config {
        cred_status.kalshi_configured = true;
        info!("[KALSHI] API key loaded: {}", &config.api_key_id[..8.min(config.api_key_id.len())]);
    } else {
        cred_status.kalshi_error = kalshi_error.clone();
        warn!("[KALSHI] ⚠️  Credentials not configured: {}", kalshi_error.as_deref().unwrap_or("Unknown error"));
        warn!("[KALSHI]    Set KALSHI_API_KEY_ID and KALSHI_PRIVATE_KEY_PATH environment variables");
    }

    // Try to load Polymarket credentials (non-fatal if missing)
    dotenvy::dotenv().ok();
    let poly_private_key = std::env::var("POLY_PRIVATE_KEY").ok();
    let poly_funder = std::env::var("POLY_FUNDER").ok();
    
    let poly_async: Option<Arc<SharedAsyncClient>> = match (&poly_private_key, &poly_funder) {
        (Some(private_key), Some(funder)) if !private_key.is_empty() && !funder.is_empty() => {
            info!("[POLYMARKET] Creating async client and deriving API credentials...");
            match PolymarketAsyncClient::new(
                POLY_CLOB_HOST,
                POLYGON_CHAIN_ID,
                private_key,
                funder,
            ) {
                Ok(client) => {
                    match client.derive_api_key(0).await {
                        Ok(api_creds) => {
                            match PreparedCreds::from_api_creds(&api_creds) {
                                Ok(prepared_creds) => {
                                    let shared = Arc::new(SharedAsyncClient::new(client, prepared_creds, POLYGON_CHAIN_ID));
                                    
                                    // Load neg_risk cache from Python script output
                                    match shared.load_cache(".clob_market_cache.json") {
                                        Ok(count) => info!("[POLYMARKET] Loaded {} neg_risk entries from cache", count),
                                        Err(e) => warn!("[POLYMARKET] Could not load neg_risk cache: {}", e),
                                    }
                                    
                                    cred_status.polymarket_configured = true;
                                    info!("[POLYMARKET] Client ready for {}...", &funder[..10.min(funder.len())]);
                                    Some(shared)
                                }
                                Err(e) => {
                                    let err_msg = format!("Failed to prepare credentials: {}", e);
                                    cred_status.polymarket_error = Some(err_msg.clone());
                                    warn!("[POLYMARKET] ⚠️  {}", err_msg);
                                    None
                                }
                            }
                        }
                        Err(e) => {
                            let err_msg = format!("Failed to derive API key: {}", e);
                            cred_status.polymarket_error = Some(err_msg.clone());
                            warn!("[POLYMARKET] ⚠️  {}", err_msg);
                            None
                        }
                    }
                }
                Err(e) => {
                    let err_msg = format!("Failed to create client: {}", e);
                    cred_status.polymarket_error = Some(err_msg.clone());
                    warn!("[POLYMARKET] ⚠️  {}", err_msg);
                    None
                }
            }
        }
        _ => {
            cred_status.polymarket_error = Some("POLY_PRIVATE_KEY or POLY_FUNDER not set".to_string());
            warn!("[POLYMARKET] ⚠️  Credentials not configured");
            warn!("[POLYMARKET]    Set POLY_PRIVATE_KEY and POLY_FUNDER environment variables");
            None
        }
    };

    // Log credential status summary
    info!("🔑 Credential status: {}", cred_status.summary());
    
    // Store credential status for web API
    let cred_status = Arc::new(cred_status);

    // Load team code mapping cache
    let team_cache = TeamCache::load();
    info!("📂 Loaded {} team code mappings", team_cache.len());

    // Build global state from discovery results
    let mut global_state = GlobalState::new();

    // Only run discovery if Kalshi credentials are available
    let kalshi_api: Option<Arc<KalshiApiClient>> = if let Some(ref config) = kalshi_config {
        let api = Arc::new(KalshiApiClient::new(config.clone()));
        
        // Run discovery (with caching support)
        let force_discovery = std::env::var("FORCE_DISCOVERY")
            .map(|v| v == "1" || v == "true")
            .unwrap_or(false);

        info!("🔍 Market discovery{}...",
              if force_discovery { " (forced refresh)" } else { "" });

        let discovery = DiscoveryClient::new(
            KalshiApiClient::new(config.clone()),
            team_cache
        );

        let result = if force_discovery {
            discovery.discover_all_force(ENABLED_LEAGUES).await
        } else {
            discovery.discover_all(ENABLED_LEAGUES).await
        };

        info!("📊 Market discovery complete:");
        info!("   - Matched market pairs: {}", result.pairs.len());

        if !result.errors.is_empty() {
            for err in &result.errors {
                warn!("   ⚠️ {}", err);
            }
        }

        if result.pairs.is_empty() {
            warn!("⚠️  No market pairs found - check credentials or wait for markets to open");
        } else {
            // Display discovered market pairs
            info!("📋 Discovered market pairs:");
            for pair in &result.pairs {
                info!("   ✅ {} | {} | Kalshi: {}",
                      pair.description,
                      pair.market_type,
                      pair.kalshi_market_ticker);
            }
            
            // Add pairs to state
            for pair in result.pairs {
                global_state.add_pair(pair);
            }
        }
        
        Some(api)
    } else {
        warn!("⚠️  Skipping market discovery - Kalshi credentials not configured");
        None
    };
    
    info!("📡 Global state initialized: tracking {} markets", global_state.market_count());
    let state = Arc::new(global_state);

    // Initialize execution infrastructure
    let (exec_tx, exec_rx) = create_execution_channel();
    let circuit_breaker = Arc::new(CircuitBreaker::new(CircuitBreakerConfig::from_env()));

    let position_tracker = Arc::new(RwLock::new(PositionTracker::new()));
    let (position_channel, position_rx) = create_position_channel();

    tokio::spawn(position_writer_loop(position_rx, position_tracker.clone()));

    let threshold_cents: PriceCents = ((ARB_THRESHOLD * 100.0).round() as u16).max(1);
    info!("   Execution threshold: {} cents", threshold_cents);

    // Only create execution engine if we have at least one platform's credentials
    if let (Some(ref kalshi), Some(ref poly)) = (&kalshi_api, &poly_async) {
        let engine = Arc::new(ExecutionEngine::new(
            kalshi.clone(),
            poly.clone(),
            state.clone(),
            circuit_breaker.clone(),
            position_channel,
            dry_run,
        ));
        tokio::spawn(run_execution_loop(exec_rx, engine));
    } else {
        warn!("⚠️  Execution engine not started - missing platform credentials");
        // Still need to consume exec_rx to avoid memory buildup
        tokio::spawn(async move {
            let mut rx = exec_rx;
            while rx.recv().await.is_some() {
                // Discard execution requests when no engine is available
            }
        });
    }

    // Start web dashboard server (always starts, regardless of credentials)
    let web_state = state.clone();
    let web_cb = circuit_breaker.clone();
    let web_tracker = position_tracker.clone();
    let web_cred_status = cred_status.clone();
    tokio::spawn(async move {
        if let Err(e) = web::start_server(web_state, web_cb, web_tracker, web_cred_status).await {
            error!("[WEB] Server failed: {}", e);
        }
    });

    // === TEST MODE: Synthetic arbitrage injection ===
    // TEST_ARB=1 to enable, TEST_ARB_TYPE=poly_yes_kalshi_no|kalshi_yes_poly_no|poly_only|kalshi_only
    let test_arb = std::env::var("TEST_ARB").map(|v| v == "1" || v == "true").unwrap_or(false);
    if test_arb {
        let test_state = state.clone();
        let test_exec_tx = exec_tx.clone();
        let test_dry_run = dry_run;

        // Parse arb type from environment (default: poly_yes_kalshi_no)
        let arb_type_str = std::env::var("TEST_ARB_TYPE").unwrap_or_else(|_| "poly_yes_kalshi_no".to_string());

        tokio::spawn(async move {
            use types::{FastExecutionRequest, ArbType};

            // Wait for WebSocket connections to establish and populate orderbooks
            info!("[TEST] Injecting synthetic arbitrage opportunity in 10 seconds...");
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

            // Parse arb type
            let arb_type = match arb_type_str.to_lowercase().as_str() {
                "poly_yes_kalshi_no" | "pykn" | "0" => ArbType::PolyYesKalshiNo,
                "kalshi_yes_poly_no" | "kypn" | "1" => ArbType::KalshiYesPolyNo,
                "poly_only" | "poly" | "2" => ArbType::PolyOnly,
                "kalshi_only" | "kalshi" | "3" => ArbType::KalshiOnly,
                _ => {
                    warn!("[TEST] Unknown TEST_ARB_TYPE='{}', defaulting to PolyYesKalshiNo", arb_type_str);
                    warn!("[TEST] Valid values: poly_yes_kalshi_no, kalshi_yes_poly_no, poly_only, kalshi_only");
                    ArbType::PolyYesKalshiNo
                }
            };

            // Set prices based on arb type for realistic test scenarios
            let (yes_price, no_price, description) = match arb_type {
                ArbType::PolyYesKalshiNo => (40, 50, "P_yes=40¢ + K_no=50¢ + fee≈2¢ = 92¢ → 8¢ profit"),
                ArbType::KalshiYesPolyNo => (40, 50, "K_yes=40¢ + P_no=50¢ + fee≈2¢ = 92¢ → 8¢ profit"),
                ArbType::PolyOnly => (48, 50, "P_yes=48¢ + P_no=50¢ + fee=0¢ = 98¢ → 2¢ profit (NO FEES!)"),
                ArbType::KalshiOnly => (44, 44, "K_yes=44¢ + K_no=44¢ + fee≈4¢ = 92¢ → 8¢ profit (DOUBLE FEES)"),
            };

            // Find first market with valid state
            let market_count = test_state.market_count();
            for market_id in 0..market_count {
                if let Some(market) = test_state.get_by_id(market_id as u16) {
                    if let Some(pair) = &market.pair {
                        // SIZE: 1000 cents = 10 contracts (Poly $1 min requires ~3 contracts at 40¢)
                        let fake_req = FastExecutionRequest {
                            market_id: market_id as u16,
                            yes_price,
                            no_price,
                            yes_size: 1000,  // 1000¢ = 10 contracts
                            no_size: 1000,   // 1000¢ = 10 contracts
                            arb_type,
                            detected_ns: 0,
                        };

                        warn!("[TEST] 🧪 Injecting synthetic {:?} arbitrage for: {}", arb_type, pair.description);
                        warn!("[TEST]    Scenario: {}", description);
                        warn!("[TEST]    Position size capped to 10 contracts for safety");
                        warn!("[TEST]    Execution mode: DRY_RUN={}", test_dry_run);

                        if let Err(e) = test_exec_tx.send(fake_req).await {
                            error!("[TEST] Failed to send fake arb: {}", e);
                        }
                        break;
                    }
                }
            }
        });
    }

    // Initialize Kalshi WebSocket connection (only if credentials available)
    let kalshi_handle = if let Some(ref config) = kalshi_config {
        let kalshi_state = state.clone();
        let kalshi_exec_tx = exec_tx.clone();
        let kalshi_threshold = threshold_cents;
        let kalshi_ws_config = config.clone();
        Some(tokio::spawn(async move {
            loop {
                if let Err(e) = kalshi::run_ws(&kalshi_ws_config, kalshi_state.clone(), kalshi_exec_tx.clone(), kalshi_threshold).await {
                    error!("[KALSHI] WebSocket disconnected: {} - reconnecting...", e);
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(WS_RECONNECT_DELAY_SECS)).await;
            }
        }))
    } else {
        info!("[KALSHI] WebSocket not started - credentials not configured");
        None
    };

    // Initialize Polymarket WebSocket connection (always starts if we have markets, doesn't need auth)
    let poly_handle = if state.market_count() > 0 {
        let poly_state = state.clone();
        let poly_exec_tx = exec_tx.clone();
        let poly_threshold = threshold_cents;
        Some(tokio::spawn(async move {
            loop {
                if let Err(e) = polymarket::run_ws(poly_state.clone(), poly_exec_tx.clone(), poly_threshold).await {
                    error!("[POLYMARKET] WebSocket disconnected: {} - reconnecting...", e);
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(WS_RECONNECT_DELAY_SECS)).await;
            }
        }))
    } else {
        info!("[POLYMARKET] WebSocket not started - no markets to monitor");
        None
    };

    // System health monitoring and arbitrage diagnostics
    let heartbeat_state = state.clone();
    let heartbeat_threshold = threshold_cents;
    let heartbeat_handle = tokio::spawn(async move {
        use crate::types::kalshi_fee_cents;
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            let market_count = heartbeat_state.market_count();
            let mut with_kalshi = 0;
            let mut with_poly = 0;
            let mut with_both = 0;
            // Track best arbitrage opportunity: (total_cost, market_id, p_yes, k_no, k_yes, p_no, fee, is_poly_yes_kalshi_no)
            let mut best_arb: Option<(u16, u16, u16, u16, u16, u16, u16, bool)> = None;

            for market in heartbeat_state.markets.iter().take(market_count) {
                let (k_yes, k_no, _, _) = market.kalshi.load();
                let (p_yes, p_no, _, _) = market.poly.load();
                let has_k = k_yes > 0 && k_no > 0;
                let has_p = p_yes > 0 && p_no > 0;
                if k_yes > 0 || k_no > 0 { with_kalshi += 1; }
                if p_yes > 0 || p_no > 0 { with_poly += 1; }
                if has_k && has_p {
                    with_both += 1;

                    let fee1 = kalshi_fee_cents(k_no);
                    let cost1 = p_yes + k_no + fee1;

                    let fee2 = kalshi_fee_cents(k_yes);
                    let cost2 = k_yes + fee2 + p_no;

                    let (best_cost, best_fee, is_poly_yes) = if cost1 <= cost2 {
                        (cost1, fee1, true)
                    } else {
                        (cost2, fee2, false)
                    };

                    if best_arb.is_none() || best_cost < best_arb.as_ref().unwrap().0 {
                        best_arb = Some((best_cost, market.market_id, p_yes, k_no, k_yes, p_no, best_fee, is_poly_yes));
                    }
                }
            }

            info!("💓 System heartbeat | Markets: {} total, {} with Kalshi prices, {} with Polymarket prices, {} with both | threshold={}¢",
                  market_count, with_kalshi, with_poly, with_both, heartbeat_threshold);

            if let Some((cost, market_id, p_yes, k_no, k_yes, p_no, fee, is_poly_yes)) = best_arb {
                let gap = cost as i16 - heartbeat_threshold as i16;
                let desc = heartbeat_state.get_by_id(market_id)
                    .and_then(|m| m.pair.as_ref())
                    .map(|p| &*p.description)
                    .unwrap_or("Unknown");
                let leg_breakdown = if is_poly_yes {
                    format!("P_yes({}¢) + K_no({}¢) + K_fee({}¢) = {}¢", p_yes, k_no, fee, cost)
                } else {
                    format!("K_yes({}¢) + P_no({}¢) + K_fee({}¢) = {}¢", k_yes, p_no, fee, cost)
                };
                if gap <= 10 {
                    info!("   📊 Best opportunity: {} | {} | gap={:+}¢ | [Poly_yes={}¢ Kalshi_no={}¢ Kalshi_yes={}¢ Poly_no={}¢]",
                          desc, leg_breakdown, gap, p_yes, k_no, k_yes, p_no);
                } else {
                    info!("   📊 Best opportunity: {} | {} | gap={:+}¢ (market efficient)",
                          desc, leg_breakdown, gap);
                }
            } else if with_both == 0 {
                warn!("   ⚠️  No markets with both Kalshi and Polymarket prices - verify WebSocket connections");
            }
        }
    });

    // Main event loop - run until termination
    info!("✅ All systems operational - entering main event loop");
    
    // Wait on all available handles
    let heartbeat_future = heartbeat_handle;
    
    match (kalshi_handle, poly_handle) {
        (Some(k), Some(p)) => {
            let _ = tokio::join!(k, p, heartbeat_future);
        }
        (Some(k), None) => {
            let _ = tokio::join!(k, heartbeat_future);
        }
        (None, Some(p)) => {
            let _ = tokio::join!(p, heartbeat_future);
        }
        (None, None) => {
            // No WebSocket connections, just run heartbeat
            info!("⚠️  Running in dashboard-only mode (no WebSocket connections)");
            let _ = heartbeat_future.await;
        }
    }

    Ok(())
}
