@echo off
REM 1M Context Enabler for Claude Desktop 3P mode
REM Adds hosts entry to block GrowthBook remote features
REM Run as Administrator!

set "HOSTS_FILE=%WINDIR%\System32\drivers\etc\hosts"
set "MARKER=# --- 1M Context Fix (added %DATE% %TIME%) ---"

REM Check if already added
findstr /C:"claude.ai" "%HOSTS_FILE%" >nul 2>&1
if %ERRORLEVEL% EQU 0 (
    echo [OK] Entry already exists in hosts file
    echo Run "enable-1m.cmd remove" to remove it
    exit /b 0
)

if "%1"=="remove" goto REMOVE

REM Add entry
echo %MARKER%>>"%HOSTS_FILE%"
echo 127.0.0.1 claude.ai>>"%HOSTS_FILE%"
echo 127.0.0.1 api.claude.ai>>"%HOSTS_FILE%"
echo.>>"%HOSTS_FILE%"

echo [OK] Added claude.ai block to hosts file
echo [OK] GrowthBook remote features will fail to load
echo [OK] Local defaults (with [1m] variants) will be used
echo.
echo Restart Claude Desktop and check model selector
echo.
echo To undo: run "enable-1m.cmd remove" as Admin
exit /b 0

:REMOVE
REM Backup first
copy "%HOSTS_FILE%" "%HOSTS_FILE%.bak" >nul 2>&1

REM Remove our entries (delete lines containing claude.ai and the marker)
findstr /V /I "claude.ai" "%HOSTS_FILE%" > "%TEMP%\hosts.tmp"
findstr /V "%MARKER%" "%TEMP%\hosts.tmp" > "%TEMP%\hosts.tmp2"
copy "%TEMP%\hosts.tmp2" "%HOSTS_FILE%" >nul 2>&1
del "%TEMP%\hosts.tmp" "%TEMP%\hosts.tmp2" 2>nul

echo [OK] Removed claude.ai block from hosts file
echo [OK] Original backed up to %HOSTS_FILE%.bak
exit /b 0
