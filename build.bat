@echo off
echo ========================================
echo   Sutty - SSH Client Build Script
echo ========================================
echo.

where cargo >nul 2>&1
if %errorlevel% neq 0 (
    echo ERROR: cargo not found. Is Rust installed?
    echo Install from https://rustup.rs
    pause
    exit /b 1
)

echo [1/2] Building TUI client (sutty.exe)...
cargo build --release -p sutty
if %errorlevel% neq 0 (
    echo ERROR: TUI build failed.
    pause
    exit /b 1
)

echo.
echo [2/2] Building GUI client (sutty-gui.exe)...
cargo build --release -p sutty-gui
if %errorlevel% neq 0 (
    echo ERROR: GUI build failed.
    pause
    exit /b 1
)

echo.
echo ========================================
echo   Build complete!
echo.
echo   TUI:  target\release\sutty.exe
echo   GUI:  target\release\sutty-gui.exe
echo ========================================
echo.
echo Press any key to open the output folder...
pause >nul
explorer target\release
