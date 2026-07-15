@echo off
chcp 65001 >nul
title My Quick Feed (DEV)

echo [DEV] Starting My Quick Feed in development mode...
set PATH=C:\msys64\mingw64\bin;%PATH%
set RUSTFLAGS=-C link-arg=-Wl,--exclude-all-symbols

cd /d "%~dp0"

echo [1/2] Starting frontend dev server...
start "Vite" cmd /c "npx vite"

echo [2/2] Starting Tauri backend (with hot reload)...
cd src-tauri
cargo run
