@echo off
set MINGW=C:\msys64\mingw64\bin
set BIN=C:\quickfeed\src-tauri\target\debug\my-quick-feed.exe

set PATH=%MINGW%;%PATH%
echo Starting...
start "My Quick Feed" /B "%BIN%"
echo PID = %ERRORLEVEL%
timeout /t 4 /nobreak >nul
tasklist /FI "IMAGENAME eq my-quick-feed.exe" 2>&1
echo.
echo If you see the process above, it works!
