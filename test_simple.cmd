@echo off
echo === My Quick Feed Test ===
set MINGW=C:\msys64\mingw64\bin
set BIN=C:\quickfeed\src-tauri\target\release\my-quick-feed.exe
echo MinGW: %MINGW%
if exist "%MINGW%\gcc.exe" (echo gcc.exe: OK) else (echo gcc.exe: MISS)
if exist "%BIN%" (echo Binary: OK) else (echo Binary: MISS)
for %%f in (libwinpthread-1.dll libgcc_s_seh-1.dll libstdc++-6.dll) do if exist "%MINGW%\%%f" (echo %%f: OK) else (echo %%f: MISS)
echo Launching...
set PATH=%MINGW%;%PATH%
"%BIN%"
echo Exit code: %ERRORLEVEL%
pause
