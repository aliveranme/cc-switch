Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  Claude Desktop Recovery" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

# Step 1: Remove hosts entry
Write-Host "[1/2] Cleaning hosts file..." -ForegroundColor Yellow
$hosts = "C:\Windows\System32\drivers\etc\hosts"
$temp = "$env:TEMP\hosts_clean.txt"
Get-Content $hosts | Where-Object {$_ -notmatch "claude\.ai"} | Set-Content $temp
Copy-Item $temp $hosts -Force
Remove-Item $temp
ipconfig /flushdns
Write-Host "[OK] Hosts cleaned" -ForegroundColor Green

# Step 2: Restore asar from backup
Write-Host "[2/2] Restoring app.asar..." -ForegroundColor Yellow
$bakDir = "$env:USERPROFILE\.claude-1m-patch\backups"
$backups = Get-ChildItem "$bakDir\*orig*" | Sort-Object Name -Descending
if ($backups.Count -gt 0) {
    $latest = $backups[0].FullName
    Write-Host "  Backup found: $($backups[0].Name)" -ForegroundColor Gray
    
    $appx = Get-AppxPackage -Name "*Claude*" -ErrorAction SilentlyContinue
    if ($appx) {
        $target = Join-Path $appx.InstallLocation "app\resources\app.asar"
        if (Test-Path $target) {
            Copy-Item $latest $target -Force
            Write-Host "[OK] app.asar restored" -ForegroundColor Green
        }
    }
} else {
    Write-Host "[WARN] No backup found - reinstall from Microsoft Store" -ForegroundColor Yellow
}

Write-Host ""
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  Done!" -ForegroundColor Cyan
Write-Host "  Restart Claude Desktop to verify" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""
Write-Host "The rebuilt cc-switch still injects context-1m header at proxy level." -ForegroundColor Gray
Write-Host "All requests will get 1M context automatically." -ForegroundColor Gray
Write-Host ""
pause
