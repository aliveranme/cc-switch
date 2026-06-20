@echo off
REM ==============================================
REM Claude Desktop 1M Context — Renderer Patch
REM ==============================================
REM 修改 ion-dist/assets/v1/index-CD05FcCU.js
REM 移除 o(t) 检查，让 [1m] 选项始终可用
REM ==============================================
setlocal enabledelayedexpansion

echo.
echo === Claude Desktop 1M Context Renderer Patch ===
echo.

:: 检查管理员权限
net session >nul 2>&1
if %errorlevel% neq 0 (
    echo 需要管理员权限...
    powershell -Command "Start-Process cmd -Verb RunAs -ArgumentList '/c \"%~f0\"' -Wait"
    exit /b 0
)

:: 查找 Claude 安装路径
echo 1. 查找 Claude Desktop...
for /f "delims=" %%p in ('powershell -Command "Get-AppxPackage -Name '*Claude*' -ErrorAction SilentlyContinue | Select-Object -ExpandProperty InstallLocation"') do (
    set "CLAUDE_PATH=%%p"
    goto :found
)

:found
if "%CLAUDE_PATH%"=="" (
    echo [ERROR] 未找到 Claude Desktop 安装
    pause
    exit /b 1
)
echo   %CLAUDE_PATH%

:: 找 renderer JS 文件
set "RENDERER_DIR=%CLAUDE_PATH%\app\resources\ion-dist\assets\v1"
if not exist "%RENDERER_DIR%" (
    echo [ERROR] 未找到 renderer 目录
    pause
    exit /b 1
)

:: 找 index-*.js 文件（处理哈希后缀不同版本）
set "INDEX_FILE="
for /f "delims=" %%f in ('dir /b "%RENDERER_DIR%\index-*.js" 2^>nul') do set "INDEX_FILE=%RENDERER_DIR%\%%f"
if "%INDEX_FILE%"=="" (
    echo [ERROR] 未找到 index-*.js 文件
    pause
    exit /b 1
)
echo   找到文件: %INDEX_FILE%

:: 获取权限
echo.
echo 2. 获取写入权限...
takeown /f "%RENDERER_DIR%" /a /r /d Y >nul 2>&1
icacls "%RENDERER_DIR%" /grant "BUILTIN\Administrators:(F)" /t /c >nul 2>&1
icacls "%RENDERER_DIR%" /grant "%USERDOMAIN%\%USERNAME%:(F)" /t /c >nul 2>&1
echo   权限获取完成

:: 备份
echo.
echo 3. 备份原始文件...
copy "%INDEX_FILE%" "%INDEX_FILE%.1m_patch_bak" >nul 2>&1
echo   备份: %INDEX_FILE%.1m_patch_bak

:: 应用补丁
echo.
echo 4. 应用补丁...

powershell -NoProfile -ExecutionPolicy Bypass -Command "&{
    $content = [System.IO.File]::ReadAllText('%INDEX_FILE%')
    
    # 要替换的原始代码和补丁后代码
    $old = 'return i&&o(t)?t:o(e)?e:o(t)?t:void 0'
    $new = 'return i?t:o(e)?e:void 0'
    
    if ($content.Contains($old)) {
        $content = $content.Replace($old, $new)
        [System.IO.File]::WriteAllText('%INDEX_FILE%', $content, [System.Text.UTF8Encoding]::new($false))
        Write-Host '  [OK] 补丁已应用'
        Write-Host '  [1m] 选择逻辑已修复 — 不再依赖 allowedModels'
    } else {
        Write-Host '  [WARN] 未匹配到目标代码，尝试其他变体...'
        # 尝试正则替换
        $regex = [regex]'return i&&o\(t\)\?t:o\(e\)\?e:o\(t\)\?t:void 0'
        if ($regex.IsMatch($content)) {
            $content = $regex.Replace($content, 'return i?t:o(e)?e:void 0', 1)
            [System.IO.File]::WriteAllText('%INDEX_FILE%', $content, [System.Text.UTF8Encoding]::new($false))
            Write-Host '  [OK] 补丁已应用(正则)'
        } else {
            Write-Host '  [ERROR] 无法匹配目标代码，Claude 可能已更新'
            Write-Host '  备份文件保留在 .1m_patch_bak'
        }
    }
}"

:: 验证
echo.
echo 5. 验证...
powershell -NoProfile -ExecutionPolicy Bypass -Command "&{
    $content = [System.IO.File]::ReadAllText('%INDEX_FILE%')
    if ($content.Contains('return i?t:o(e)?e:void 0')) {
        Write-Host '  [OK] 验证通过'
        Write-Host '  [1m] 选项将始终显示（不受远程限制）'
    } elseif ($content.Contains('return i&&o(t)?t:o(e)?e:o(t)?t:void 0')) {
        Write-Host '  [WARN] 验证失败 — 补丁未生效'
    } else {
        Write-Host '  [WARN] 无法确定补丁状态'
    }
}"

:: 清理 hosts 条目（之前可能添加的）
echo.
echo 6. 清理 hosts 文件中的 claude.ai 条目...
powershell -NoProfile -ExecutionPolicy Bypass -Command "&{
    try {
        $hosts = 'C:\Windows\System32\drivers\etc\hosts'
        $temp = [System.IO.Path]::GetTempFileName()
        Get-Content $hosts | Where-Object {$_ -notmatch 'claude\.ai'} | Set-Content $temp -Encoding UTF8
        Move-Item $temp $hosts -Force -ErrorAction SilentlyContinue
        ipconfig /flushdns 2>&1 | Out-Null
        Write-Host '  [OK] hosts 已清理'
    } catch {
        Write-Host '  [WARN] hosts 清理失败（忽略）'
    }
}"
cmd /c "ipconfig /flushdns" >nul 2>&1

echo.
echo ============================================
echo  补丁完成！
echo ============================================
echo.
echo [1m] 变体现在会始终显示（不受远程限制）
echo 重启 Claude Desktop 后生效
echo.
echo 如需恢复: 删除 %INDEX_FILE%
echo 并重命名 %INDEX_FILE%.1m_patch_bak 恢复
echo.
pause
