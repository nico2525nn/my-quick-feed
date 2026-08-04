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

rem 起動中の旧アプリを停止（exe がロックされると cargo build が失敗するため）
taskkill /IM my-quick-feed.exe /F >nul 2>&1

echo [1/3] Building frontend...
call npx vite build
if errorlevel 1 (
    echo [ERROR] Frontend build failed
    pause
    exit /b 1
)

echo [2/3] Building backend...
pushd src-tauri
call cargo build --release
popd
if errorlevel 1 (
    echo [ERROR] Backend build failed
    pause
    exit /b 1
)

echo [3/3] Launching...
set "BIN=src-tauri\target\release\my-quick-feed.exe"
start "" /B "%BIN%"
echo [OK] My Quick Feed started (system tray)
