@echo off
chcp 65001 >nul
cd /d "%~dp0"

set BIN=src-tauri\target\release\my-quick-feed.exe
set MINGW=C:\msys64\mingw64\bin

if not exist "%BIN%" (
    if not exist "%MINGW%\gcc.exe" (
        echo MinGW not found at %MINGW%
        pause
        exit /b 1
    )
    echo Building binary...
    set "PATH=%MINGW%;%PATH%"
    pushd src-tauri
    call cargo build --release
    popd
    if errorlevel 1 (
        echo Build failed (install Rust: https://rustup.rs)
        pause
        exit /b 1
    )
)

set "PATH=%MINGW%;%PATH%"
start "" /B "%BIN%"
echo My Quick Feed started (system tray)
