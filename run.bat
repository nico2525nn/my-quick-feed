@echo off
rem Wrapper: real build script lives at D:\quickfeed\run.bat (ASCII path required for Rust build)
rem %* で引数を本体に渡す（2026-08-05 修正: これが無いと --no-run 等が消える）
echo [INFO] Running D:\quickfeed\run.bat %*
call "D:\quickfeed\run.bat" %*
