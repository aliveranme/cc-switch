@echo off
REM Remove claude.ai from hosts — Run as Administrator
setlocal
set "HOSTS=%WINDIR%\System32\drivers\etc\hosts"
set "TEMP_FILE=%TEMP%\hosts_clean.txt"

REM Backup
copy "%HOSTS%" "%HOSTS%.bak" >nul 2>&1

REM Remove claude.ai lines
findstr /V /I "claude.ai" "%HOSTS%" > "%TEMP_FILE%"
if %ERRORLEVEL% NEQ 0 (
    echo [ERROR] Failed to filter hosts file
    exit /b 1
)

copy /Y "%TEMP_FILE%" "%HOSTS%" >nul 2>&1
if %ERRORLEVEL% EQU 0 (
    echo [OK] Removed claude.ai from hosts file
) else (
    echo [ERROR] Access denied. Run as Administrator!
    echo Right-click this file and select "Run as administrator"
    exit /b 1
)

del "%TEMP_FILE%" 2>nul

REM Verify
findstr "claude.ai" "%HOSTS%" >nul 2>&1
if %ERRORLEVEL% EQU 0 (
    echo [WARN] Entry still present
) else (
    echo [OK] Verified: claude.ai entry removed
)

ipconfig /flushdns >nul 2>&1
echo [OK] DNS cache flushed
