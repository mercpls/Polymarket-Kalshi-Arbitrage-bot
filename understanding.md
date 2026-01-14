# Polymarket-Kalshi Arbitrage Bot - Technical Understanding Report

## Executive Summary

The Polymarket-Kalshi Arbitrage Bot is a sophisticated, high-performance trading system written in Rust that automatically identifies and executes risk-free arbitrage opportunities between two prediction market platforms: Kalshi (regulated) and Polymarket (decentralized). The system operates 24/7, monitoring price discrepancies and executing trades when profitable opportunities arise.

## Core Trading Strategy

### Arbitrage Principle

The fundamental arbitrage opportunity exploits the mathematical property of prediction markets: **YES + NO = $1.00** (guaranteed payout). Arbitrage exists when:

```
Best YES ask (Platform A) + Best NO ask (Platform B) < $1.00
```

**Example Scenario:**
- Kalshi YES ask: 42¢
- Polymarket NO ask: 56¢
- Total cost: 98¢
- Guaranteed payout: 100¢
- Risk-free profit: 2¢ per contract

### Four Arbitrage Types

1. **poly_yes_kalshi_no**: Buy Polymarket YES, Sell Kalshi NO
2. **kalshi_yes_poly_no**: Buy Kalshi YES, Sell Polymarket NO  
3. **poly_same_market**: Both legs on Polymarket (rare, no fees)
4. **kalshi_same_market**: Both legs on Kalshi (rare, double fees)

### Fee Structure

- **Kalshi**: `ceil(0.07 × contracts × price × (1-price))` - factored into detection
- **Polymarket**: Zero trading fees

## System Architecture

### High-Level Design

```
┌─────────────────┐    ┌─────────────────┐
│   Kalshi API    │    │ Polymarket API  │
│                 │    │                 │
│  REST + WS      │    │  WebSocket      │
└─────────┬───────┘    └─────────┬───────┘
          │                      │
          └──────────┬───────────┘
                     │
        ┌────────────▼────────────┐
        │    Global State        │
        │  (Lock-free Orderbooks) │
        └────────────┬────────────┘
                     │
        ┌────────────▼────────────┐
        │  Arbitrage Detection    │
        │   (SIMD Accelerated)    │
        └────────────┬────────────┘
                     │
        ┌────────────▼────────────┐
        │   Execution Engine      │
        │ (Concurrent Order Exec) │
        └────────────┬────────────┘
                     │
        ┌────────────▼────────────┐
        │  Risk Management        │
        │ (Circuit Breaker)       │
        └─────────────────────────┘
```

### Core Components

#### 1. Market Discovery (`discovery.rs`)
- **Purpose**: Intelligently matches equivalent markets between platforms
- **Features**: 
  - Rate-limited API calls (Kalshi: 2 req/sec, Polymarket: 20 concurrent)
  - Persistent caching (2-hour TTL)
  - Support for multiple sports leagues (EPL, NBA, NFL, NHL, MLB, etc.)
  - Parallel processing with semaphores

#### 2. Global State Management (`types.rs`)
- **AtomicOrderbook**: Lock-free orderbook using packed 64-bit integers
  - Bit layout: `[yes_ask:16][no_ask:16][yes_size:16][no_size:16]`
  - Zero-copy updates via atomic operations
  - Cache-line aligned for performance
- **GlobalState**: Centralized market state tracking up to 1024 markets

#### 3. Real-time Data Feeds
- **Kalshi WebSocket** (`kalshi.rs`): Real-time price updates and order execution
- **Polymarket WebSocket** (`polymarket.rs`): CLOB data stream and order book management
- **Auto-reconnection**: 5-second reconnect delay with error handling

#### 4. Arbitrage Detection Engine
- **SIMD Acceleration**: Uses wide SIMD instructions for sub-millisecond detection
- **Threshold-based**: Default 0.5% profit threshold (configurable)
- **Real-time scanning**: Continuous monitoring of all market pairs

#### 5. Execution Engine (`execution.rs`)
- **Concurrent Execution**: Parallel order placement across platforms
- **In-flight Deduplication**: Prevents duplicate orders using atomic counters
- **Latency Tracking**: Nanosecond precision timing for performance analysis
- **Position Reconciliation**: Automatic matching of filled orders

#### 6. Risk Management (`circuit_breaker.rs`)
- **Position Limits**: Per-market and total position caps
- **Loss Limits**: Daily loss thresholds with automatic halt
- **Error Tracking**: Consecutive error monitoring
- **Cooldown Periods**: Automatic recovery after circuit breaker trips

#### 7. Position Tracking (`position_tracker.rs`)
- **Real-time P&L**: Live profit and loss calculation
- **Cost Basis Tracking**: FIFO accounting for position management
- **Persistent Storage**: JSON-based position persistence
- **Channel-based Updates**: Async fill recording system

## Technical Implementation Details

### Performance Optimizations

1. **Lock-free Data Structures**: Atomic operations for orderbook updates
2. **SIMD Processing**: Vectorized arbitrage calculations
3. **Memory Alignment**: Cache-line optimized structures
4. **Zero-copy Architecture**: Minimized memory allocations
5. **Async I/O**: Non-blocking network operations

### Key Data Structures

```rust
// Atomic orderbook (64-bit packed)
pub struct AtomicOrderbook {
    packed: AtomicU64,  // [yes_ask:16][no_ask:16][yes_size:16][no_size:16]
}

// Market pair mapping
pub struct MarketPair {
    pub pair_id: Arc<str>,
    pub league: Arc<str>,
    pub market_type: MarketType,
    pub description: Arc<str>,
    // ... platform-specific identifiers
}

// Execution request
pub struct FastExecutionRequest {
    pub market_id: u16,
    pub yes_price: u16,
    pub no_price: u16,
    pub yes_size: u16,
    pub no_size: u16,
    pub arb_type: ArbType,
    pub detected_ns: u64,
}
```

### Configuration System

#### Environment Variables
- **Credentials**: `KALSHI_API_KEY_ID`, `KALSHI_PRIVATE_KEY_PATH`, `POLY_PRIVATE_KEY`, `POLY_FUNDER`
- **Bot Configuration**: `DRY_RUN`, `RUST_LOG`, `FORCE_DISCOVERY`, `PRICE_LOGGING`
- **Circuit Breaker**: `CB_ENABLED`, `CB_MAX_POSITION_PER_MARKET`, `CB_MAX_DAILY_LOSS`
- **Test Mode**: `TEST_ARB`, `TEST_ARB_TYPE`

#### League Support
- **Soccer**: EPL, Bundesliga, La Liga, Serie A, Ligue 1, UCL, UEL, EFL Championship
- **Basketball**: NBA
- **Football**: NFL
- **Hockey**: NHL
- **Baseball**: MLB, MLS
- **College Football**: NCAAF

## Operational Flow

### 1. Initialization Phase
1. Load credentials and configuration
2. Initialize API clients for both platforms
3. Load team code cache for market matching
4. Run market discovery with caching
5. Build global state with discovered market pairs
6. Initialize execution infrastructure

### 2. Runtime Phase
1. **WebSocket Connections**: Establish real-time data feeds
2. **Price Updates**: Continuously update atomic orderbooks
3. **Arbitrage Scanning**: Real-time detection using SIMD
4. **Opportunity Execution**: Concurrent order placement
5. **Position Tracking**: Fill reconciliation and P&L updates
6. **Risk Monitoring**: Circuit breaker enforcement

### 3. Monitoring & Diagnostics
- **Heartbeat System**: 60-second status reports
- **Best Opportunity Tracking**: Real-time gap analysis
- **System Health**: Connection status and error monitoring
- **Performance Metrics**: Latency and throughput tracking

## Safety Features

### Circuit Breaker Protections
- Maximum position per market: 100 contracts (default)
- Maximum total position: 500 contracts (default)
- Maximum daily loss: $5,000 (default)
- Maximum consecutive errors: 5 (default)
- Cooldown period: 60 seconds (default)

### Operational Safeguards
- **Dry Run Mode**: Paper trading without real money
- **Test Mode**: Synthetic arbitrage injection for testing
- **Rate Limiting**: API protection against throttling
- **Error Recovery**: Automatic reconnection and state recovery
- **Position Limits**: Prevents overexposure

## Development & Testing

### Build System
- **Language**: Rust (edition 2021)
- **Optimization**: Release build with LTO and codegen-units=1
- **Dependencies**: Async ecosystem (tokio), WebSockets, cryptography
- **Testing**: Unit tests, integration tests, benchmarks

### Test Modes
- **Synthetic Arbitrage**: `TEST_ARB=1` injects fake opportunities
- **Multiple Types**: Test different arbitrage scenarios
- **Safe Execution**: Capped position sizes for testing

## Performance Characteristics

### Latency
- **Detection**: Sub-millisecond arbitrage identification
- **Execution**: Concurrent order placement with nanosecond tracking
- **Updates**: Real-time WebSocket price feeds

### Throughput
- **Markets**: Up to 1024 concurrent market tracking
- **Updates**: Lock-free atomic operations
- **Scanning**: Continuous SIMD-accelerated processing

### Reliability
- **Auto-reconnection**: 5-second reconnect delay
- **State Persistence**: Position and cache persistence
- **Error Handling**: Comprehensive error recovery

## Market Coverage

The bot supports multiple sports leagues with intelligent market matching:
- **European Soccer**: Major leagues with full market types (moneyline, spread, total, BTTS)
- **American Sports**: NBA, NFL, NHL, MLB with comprehensive market coverage
- **College Sports**: NCAAF support
- **Market Types**: Moneyline, point spreads, totals, both teams to score

## Economic Model

### Profit Sources
1. **Cross-platform Price Discrepancies**: Primary arbitrage opportunities
2. **Market Inefficiencies**: Temporary pricing imbalances
3. **Fee Arbitrage**: Exploiting different fee structures

### Cost Considerations
- **Kalshi Fees**: 7% of max exposure, factored into detection
- **Gas Fees**: Ethereum network costs for Polymarket
- **Slippage**: Market impact during execution

## Future Enhancements

### Planned Features
- Risk limit configuration UI
- Multi-account support
- Advanced order routing strategies
- Historical performance analytics dashboard

### Scalability Considerations
- Additional platform support
- More market types
- Enhanced detection algorithms
- Machine learning integration

## Conclusion

The Polymarket-Kalshi Arbitrage Bot represents a sophisticated, production-ready trading system that combines high-performance computing with financial engineering. Its lock-free architecture, SIMD acceleration, and comprehensive risk management make it suitable for 24/7 automated trading operations.

The system is designed with both beginners and advanced users in mind, offering extensive documentation, test modes, and configurable safety features while maintaining the performance characteristics required for competitive arbitrage trading.

**Key Strengths:**
- High-performance, low-latency execution
- Comprehensive risk management
- Beginner-friendly with extensive documentation
- Production-ready with robust error handling
- Extensible architecture for future enhancements

**Technical Excellence:**
- Modern Rust implementation with async/await
- Lock-free data structures for maximum performance
- SIMD-accelerated arbitrage detection
- Comprehensive testing and monitoring capabilities
