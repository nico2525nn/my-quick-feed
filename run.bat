@echo off
chcp 65001 >nul
title My Quick Feed

set BIN=src-tauri\target\debug\my-quick-feed.exe

if not exist "%BIN%" (
    echo [BUILD] Building binary...
    set PATH=C:\msys64\mingw64\bin;%PATH%
    cd src-tauri
    call cargo build
    cd ..
    if errorlevel 1 (
        echo [ERROR] Build failed
        pause
        exit /b 1
    )
)

echo [LAUNCH] Starting My Quick Feed...
set PATH=C:\msys64\mingw64\bin;%PATH%
start /b "" "%BIN%"
echo [OK] Running in system tray
