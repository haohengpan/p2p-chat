# P2P Chat - Windows Build Script
# Run this from PowerShell on the Windows side.
#
# Prerequisites:
#   winget install Rustlang.Rustup
#   winget install OpenJS.NodeJS.LTS
#   winget install Microsoft.VisualStudio.2022.BuildTools --override "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
#
# Usage:
#   Option 1: Build directly from WSL2 filesystem (slower I/O):
#     cd \\wsl$\Ubuntu\root\workspace\claude\p2p-chat
#     .\scripts\build-windows.ps1
#
#   Option 2: Copy to Windows first (faster):
#     Copy-Item -Recurse \\wsl$\Ubuntu\root\workspace\claude\p2p-chat C:\projects\p2p-chat
#     cd C:\projects\p2p-chat
#     .\scripts\build-windows.ps1

$ErrorActionPreference = "Stop"

Write-Host "=== P2P Chat Windows Build ===" -ForegroundColor Cyan

# Check prerequisites
Write-Host "`nChecking prerequisites..." -ForegroundColor Yellow

$missing = @()
if (-not (Get-Command rustc -ErrorAction SilentlyContinue)) { $missing += "Rust (winget install Rustlang.Rustup)" }
if (-not (Get-Command node -ErrorAction SilentlyContinue)) { $missing += "Node.js (winget install OpenJS.NodeJS.LTS)" }
if (-not (Get-Command npm -ErrorAction SilentlyContinue)) { $missing += "npm (comes with Node.js)" }

if ($missing.Count -gt 0) {
    Write-Host "Missing prerequisites:" -ForegroundColor Red
    $missing | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
    exit 1
}

Write-Host "  Rust: $(rustc --version)" -ForegroundColor Green
Write-Host "  Node: $(node --version)" -ForegroundColor Green
Write-Host "  npm:  $(npm --version)" -ForegroundColor Green

# Install frontend dependencies
Write-Host "`nInstalling frontend dependencies..." -ForegroundColor Yellow
npm install
if ($LASTEXITCODE -ne 0) { Write-Host "npm install failed" -ForegroundColor Red; exit 1 }

# Build
Write-Host "`nBuilding Tauri app for Windows..." -ForegroundColor Yellow
npx tauri build
if ($LASTEXITCODE -ne 0) { Write-Host "Build failed" -ForegroundColor Red; exit 1 }

# Show output
Write-Host "`n=== Build complete ===" -ForegroundColor Green
$bundlePath = "src-tauri\target\release\bundle"
Write-Host "Output files:" -ForegroundColor Cyan

if (Test-Path "$bundlePath\nsis") {
    Get-ChildItem "$bundlePath\nsis\*.exe" | ForEach-Object {
        Write-Host "  NSIS: $($_.FullName)" -ForegroundColor White
    }
}
if (Test-Path "$bundlePath\msi") {
    Get-ChildItem "$bundlePath\msi\*.msi" | ForEach-Object {
        Write-Host "  MSI:  $($_.FullName)" -ForegroundColor White
    }
}
