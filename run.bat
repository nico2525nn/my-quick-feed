@echo off
chcp 65001 >nul
title My Quick Feed

set BIN=src-tauri\target\release\my-quick-feed.exe
set BIN_DEBUG=src-tauri\target\debug\my-quick-feed.exe
set MINGW=C:\msys64\mingw64\bin

if not exist "%MINGW%\gcc.exe" (
    echo [ERROR] MinGW not found at %MINGW%
    pause
    exit /b 1
)

if not exist "%BIN%" (
    echo [BUILD] Building release binary...
    set PATH=%MINGW%;%PATH%
    pushd src-tauri
    call cargo build --release
    popd
    if errorlevel 1 (
        echo [ERROR] Build failed
        pause
        exit /b 1
    )
)

echo [LAUNCH] My Quick Feed を起動します...
set PATH=%MINGW%;%PATH%
start "My Quick Feed" /B "%BIN%" > nul 2>&1
echo [OK] タスクトレイに常駐しました
