@echo off
chcp 65001 >nul
cd /d "%~dp0"

set MINGW=C:\msys64\mingw64\bin

if not exist "%MINGW%\gcc.exe" (
    echo [ERROR] MinGW not found at %MINGW%
    pause
    exit /b 1
)

set "PATH=%MINGW%;%PATH%"

rem --dev: debug build for development (fastest, ~14s incremental)
rem release build is ~20s (incremental profile)
rem App ignores unknown flags, so --dev can be passed through.
set "DEV_MODE=0"
for %%a in (%*) do if /i "%%a"=="--dev" set "DEV_MODE=1"

rem Kill old app first (exe lock breaks cargo build)
taskkill /IM my-quick-feed.exe /F >nul 2>&1

echo [1/3] Building frontend...
call npx vite build
if errorlevel 1 (
    echo [ERROR] Frontend build failed
    pause
    exit /b 1
)

if "%DEV_MODE%"=="1" (
    set "PROFILE=debug"
    set "CARGO_FLAG="
) else (
    set "PROFILE=release"
    set "CARGO_FLAG=--release"
)
echo [2/3] Building backend (%PROFILE%)...
pushd src-tauri
call cargo build %CARGO_FLAG%
popd
if errorlevel 1 (
    echo [ERROR] Backend build failed
    pause
    exit /b 1
)

echo [3/3] Launching (%PROFILE%)...
set "BIN=src-tauri\target\%PROFILE%\my-quick-feed.exe"
start "" /B "%BIN%" %*
echo [OK] My Quick Feed started (system tray, %PROFILE%)
