@echo off
setlocal

echo ===================================================
echo   Polymarket-Kalshi Arbitrage Bot - Setup Script
echo ===================================================

REM Check for Node.js
where node >nul 2>nul
if %errorlevel% neq 0 (
    echo [ERROR] Node.js is not installed or not in PATH.
    echo Please install Node.js from https://nodejs.org/
    pause
    exit /b 1
)

REM Check for Rust
where cargo >nul 2>nul
if %errorlevel% neq 0 (
    echo [ERROR] Rust/Cargo is not installed or not in PATH.
    echo Please install Rust from https://rustup.rs/
    pause
    exit /b 1
)

echo.
echo [1/4] Installing Frontend Dependencies...
cd dashboard
call npm install
if %errorlevel% neq 0 (
    echo [ERROR] Failed to install frontend dependencies.
    pause
    exit /b 1
)

echo.
echo [2/4] Building Frontend...
call npm run build
if %errorlevel% neq 0 (
    echo [ERROR] Failed to build frontend.
    pause
    exit /b 1
)
cd ..

echo.
echo [3/4] Building Rust Backend...
cargo build --release
if %errorlevel% neq 0 (
    echo [ERROR] Failed to build backend.
    pause
    exit /b 1
)

echo.
echo [4/4] Starting Application in Demo Mode...
echo.
echo     - Mode: PAPER TRADING (Dry Run)
echo     - Max Daily Loss: $100
echo.

REM Start Backend in a new window
echo Starting Backend Server on port 8080...
start "Arbitrage Bot Backend" cmd /k "title Arbitrage Bot Backend && set DRY_RUN=1 && set CB_MAX_DAILY_LOSS=100 && set RUST_LOG=info && cargo run --release"

REM Start Frontend Dev Server in a new window
echo Starting Frontend Dev Server on port 5173...
cd dashboard
start "Dashboard Frontend" cmd /k "title Dashboard Frontend && npm run dev"
cd ..

echo.
echo ===================================================
echo   Setup Complete!
echo ===================================================
echo.
echo 1. Backend API is running at http://localhost:8080
echo 2. Frontend Dev Server is starting at http://localhost:5173
echo.
echo Opening Dashboard in browser...
timeout /t 5 >nul
start http://localhost:5173

pause
