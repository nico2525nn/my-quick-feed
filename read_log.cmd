@echo off
set LOGDIR=%APPDATA%\com.myquickfeed.app\logs
echo === Log files in %LOGDIR% ===
dir /b "%LOGDIR%"
echo.
echo === Latest log ===
for /f "delims=" %%f in ('dir /b /o-d "%LOGDIR%\*.log"') do (
    type "%LOGDIR%\%%f"
    goto :eof
)
