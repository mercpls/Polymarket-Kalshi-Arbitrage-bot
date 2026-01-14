#!/bin/bash
set -e

echo "==================================================="
echo "  Polymarket-Kalshi Arbitrage Bot - Setup Script"
echo "==================================================="

# Check requirements
if ! command -v node &> /dev/null; then
    echo "[ERROR] Node.js is not installed."
    exit 1
fi

if ! command -v cargo &> /dev/null; then
    echo "[ERROR] Rust/Cargo is not installed."
    exit 1
fi

echo ""
echo "[1/4] Installing Frontend Dependencies..."
cd dashboard
npm install

echo ""
echo "[2/4] Building Frontend..."
npm run build
cd ..

echo ""
echo "[3/4] Building Rust Backend..."
cargo build --release

echo ""
echo "[4/4] Starting Application in Demo Mode..."
echo "    - Mode: PAPER TRADING (Dry Run)"
echo "    - Max Daily Loss: $100"
echo ""

# Function to kill child processes on exit
cleanup() {
    echo "Shutting down..."
    kill $(jobs -p) 2>/dev/null
}
trap cleanup EXIT

# Start Backend
echo "Starting Backend Server on port 8080..."
export DRY_RUN=1
export CB_MAX_DAILY_LOSS=100
export RUST_LOG=info
cargo run --release &
BACKEND_PID=$!

# Start Frontend
echo "Starting Frontend Dev Server on port 5173..."
cd dashboard
npm run dev &
FRONTEND_PID=$!
cd ..

echo ""
echo "==================================================="
echo "  Setup Complete!"
echo "==================================================="
echo ""
echo "Backend PID: $BACKEND_PID"
echo "Frontend PID: $FRONTEND_PID"
echo ""
echo "Open http://localhost:5173 to view the dashboard"
echo ""
echo "Press Ctrl+C to stop all servers."

# Wait for background processes
wait
