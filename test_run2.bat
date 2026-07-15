@echo off
chcp 65001 >nul
setlocal enabledelayedexpansion

echo === My Quick Feed テスト起動 ===
echo.

set MINGW=C:\msys64\mingw64\bin
set BIN=C:\quickfeed\src-tauri\target\release\my-quick-feed.exe

echo [1] Checking MinGW: %MINGW%\gcc.exe
if exist "%MINGW%\gcc.exe" (echo   OK) else (echo   MISSING & pause & exit /b)

echo [2] Checking binary: %BIN%
if exist "%BIN%" (echo   OK) else (echo   MISSING & pause & exit /b)

echo [3] Checking DLLs:
for %%d in (libwinpthread-1.dll libgcc_s_seh-1.dll libstdc++-6.dll) do (
    if exist "%MINGW%\%%d" (echo   %%d - OK) else (echo   %%d - MISSING)
)

echo [4] Setting PATH and launching...
set PATH=%MINGW%;%PATH%
echo   PATH includes MinGW: OK

echo [5] Starting binary (独占モード - 閉じると終了)...
echo.
echo ===== APP OUTPUT =====
"%BIN%"
echo ===== APP EXITED =====
echo.
echo 終了コード: %ERRORLEVEL%
pause
