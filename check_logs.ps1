$logDir = "$env:APPDATA\com.myquickfeed.app\logs"
Write-Output "=== Log files ==="
if (Test-Path $logDir) {
    Get-ChildItem $logDir | Select-Object Name, Length, LastWriteTime | Format-Table -AutoSize
    $latest = Get-ChildItem $logDir | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if ($latest) {
        Write-Output "=== Latest: $($latest.Name) ==="
        Get-Content $latest.FullName -Tail 50
    }
} else {
    Write-Output "No logs directory found"
}
