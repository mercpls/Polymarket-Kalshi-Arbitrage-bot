<#
.SYNOPSIS
    Setup and run script for the Polymarket-Kalshi Arbitrage Bot.
.DESCRIPTION
    Installs dependencies, builds the project, and launches the backend 
    and frontend in demo mode.

    EMOJIS BREAK COMPILATION
#>

$ErrorActionPreference = "Stop"

# Helper functions for UI
function Show-Banner {
    Clear-Host
    Write-Host "===================================================" -ForegroundColor Cyan
    Write-Host "   Polymarket-Kalshi Arbitrage Bot - Setup" -ForegroundColor White
    Write-Host "===================================================" -ForegroundColor Cyan
    Write-Host ""
}

function Invoke-Step {
    param(
        [string]$Name,
        [scriptblock]$Action,
        [int]$Step,
        [int]$TotalSteps
    )
    
    $percent = ($Step / $TotalSteps) * 100
    Write-Progress -Activity "Setup in progress" -Status "Step $Step of ${TotalSteps}: $Name" -PercentComplete $percent
    
    Write-Host "[$Step/$TotalSteps] $Name..." -ForegroundColor Yellow -NoNewline
    
    try {
        & $Action
        Write-Host " Done" -ForegroundColor Green
    }
    catch {
        Write-Host " Failed" -ForegroundColor Red
        Write-Host "Error: $_" -ForegroundColor Red
        exit 1
    }
}

Show-Banner

# 1. Check Requirements
Write-Host "Checking requirements..." -ForegroundColor Gray
if (Get-Command node -ErrorAction SilentlyContinue) {
    Write-Host " Node.js found: $(node --version)" -ForegroundColor DarkGray
}
else {
    Write-Host " Node.js not found. Please install from https://nodejs.org/" -ForegroundColor Red
    exit 1
}

if (Get-Command cargo -ErrorAction SilentlyContinue) {
    Write-Host " Cargo found: $(cargo --version | Select-Object -First 1)" -ForegroundColor DarkGray
}
else {
    Write-Host " Rust/Cargo not found. Please install from https://rustup.rs/" -ForegroundColor Red
    exit 1
}
Write-Host ""

$TotalSteps = 4

# 2. Install Frontend Dependencies
Invoke-Step -Name "Installing Frontend Dependencies" -Step 1 -TotalSteps $TotalSteps -Action {
    Push-Location "dashboard"
    # Capture output to avoid clutter, only show on error
    $output = npm install 2>&1
    if ($LASTEXITCODE -ne 0) { throw $output }
    Pop-Location
}

# 3. Build Frontend
Invoke-Step -Name "Building Frontend" -Step 2 -TotalSteps $TotalSteps -Action {
    Push-Location "dashboard"
    $output = npm run build 2>&1
    if ($LASTEXITCODE -ne 0) { throw $output }
    Pop-Location
}

# 4. Build Backend
Invoke-Step -Name "Building Rust Backend" -Step 3 -TotalSteps $TotalSteps -Action {
    $ErrorActionPreference = "Continue"
    $output = cargo build --release 2>&1 | Out-String
    $ErrorActionPreference = "Stop"
    # Check for "Finished" in output since warnings on stderr cause false exit codes
    if ($output -notmatch "Finished.*release") { throw $output }
}

# 5. Launch
Invoke-Step -Name "Launching Application" -Step 4 -TotalSteps $TotalSteps -Action {
    Write-Host "`n`nStarting processes in separate windows..." -ForegroundColor Cyan
    
    # Start Backend
    $backendArgs = '-NoExit', '-Command', '$Host.UI.RawUI.WindowTitle = \"Arbitrage Bot Backend\"; $env:DRY_RUN=\"1\"; $env:CB_MAX_DAILY_LOSS=\"100\"; $env:RUST_LOG=\"info\"; Write-Host \"Starting Backend...\" -ForegroundColor Green; cargo run --release'
    Start-Process powershell -ArgumentList $backendArgs
    
    # Start Frontend Dev Server
    $frontendArgs = '-NoExit', '-Command', '$Host.UI.RawUI.WindowTitle = \"Dashboard Frontend\"; Set-Location dashboard; Write-Host \"Starting Frontend Dev Server...\" -ForegroundColor Green; npm run dev'
    Start-Process powershell -ArgumentList $frontendArgs
}

Write-Progress -Completed -Activity "Setup in progress"

Write-Host ""
Write-Host "===================================================" -ForegroundColor Cyan
Write-Host "   Setup Complete!" -ForegroundColor Green
Write-Host "===================================================" -ForegroundColor Cyan
Write-Host ""
Write-Host "1. Backend API is running at http://localhost:8080" -ForegroundColor Gray
Write-Host "2. Frontend Dev Server is starting at http://localhost:5173" -ForegroundColor Gray
Write-Host ""
Write-Host "Opening Dashboard in browser..." -ForegroundColor Yellow

Start-Sleep -Seconds 3
Start-Process "http://localhost:5173"
