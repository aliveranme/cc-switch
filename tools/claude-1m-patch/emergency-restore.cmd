@echo off
REM ========================================
REM 紧急恢复脚本 - 以管理员身份运行
REM 1. 移除 hosts 中的 claude.ai
REM 2. 从备份还原 app.asar
REM ========================================
setlocal enabledelayedexpansion

echo ═══════════════════════════════════════
echo   Claude Desktop 紧急恢复
echo ═══════════════════════════════════════
echo.

:: 提权检查
net session >nul 2>&1
if %errorlevel% neq 0 (
    echo [ERROR] 请以管理员身份运行此脚本
    echo 右键单击 → "以管理员身份运行"
    pause
    exit /b 1
)

:: Step 1: 恢复 hosts
echo [1/2] 恢复 hosts 文件...
set "HOSTS=%WINDIR%\System32\drivers\etc\hosts"
set "TEMP_FILE=%TEMP%\hosts_restore.txt"

copy "%HOSTS%" "%HOSTS%.bak" >nul 2>&1
findstr /V /I "claude.ai" "%HOSTS%" > "%TEMP_FILE%"
copy /Y "%TEMP_FILE%" "%HOSTS%" >nul 2>&1
del "%TEMP_FILE%" 2>nul
echo [OK] hosts 已清理
ipconfig /flushdns >nul 2>&1

:: Step 2: 还原 asar
echo [2/2] 还原 app.asar...

:: 查找最新备份
set "BACKUP_DIR=%USERPROFILE%\.claude-1m-patch\backups"
set LATEST_BACKUP=

for /f "delims=" %%f in ('dir /b /o-d "%BACKUP_DIR%\*orig*" 2^>nul') do (
    set "LATEST_BACKUP=%%f"
    goto :found_backup
)

:found_backup
if "%LATEST_BACKUP%"=="" (
    echo [WARN] 未找到备份，需重新安装 Claude Desktop
    echo 请去 Microsoft Store 重新安装
) else (
    :: 查找当前 asar 路径
    for /f "delims=" %%p in ('powershell -Command "Get-AppxPackage -Name '*Claude*' -ErrorAction SilentlyContinue | Select-Object -ExpandProperty InstallLocation" 2^>nul') do (
        set "ASAR_TARGET=%%p\app\resources\app.asar"
        if exist "!ASAR_TARGET!" (
            copy /Y "%BACKUP_DIR%\%LATEST_BACKUP%" "!ASAR_TARGET!" >nul 2>&1
            if errorlevel 1 (
                echo [WARN] 还原失败，请手动复制：
                echo  来源: %BACKUP_DIR%\%LATEST_BACKUP%
                echo  目标: !ASAR_TARGET!
            ) else (
                echo [OK] asar 已恢复
            )
        )
    )
)

echo.
echo ═══════════════════════════════════════
echo   恢复完成！
echo   请启动 Claude Desktop 测试
echo ═══════════════════════════════════════
echo.
echo 注意：之前编译的 cc-switch 已包含
echo context-1m-2025-08-07 头注入。
echo 重启 cc-switch 后所有请求自动获得
echo 1M 上下文（代理层生效，无需 UI 选择）。
echo.
pause
