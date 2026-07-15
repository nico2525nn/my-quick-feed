@echo off
chcp 65001 >nul
cd /d "%~dp0"

set MINGW=C:\msys64\mingw64\bin
set "PATH=%MINGW%;%PATH%"

echo [DEV] npx tauri dev (hot-reload)
npx tauri dev
pause
