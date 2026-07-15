$bin = "C:\quickfeed\src-tauri\target\debug\my-quick-feed.exe"
Write-Output "Starting $bin ..."
$p = Start-Process -PassThru -FilePath $bin -WindowStyle Hidden
Start-Sleep -Seconds 3
if (-not $p.HasExited) {
    Write-Output "RUNNING (PID: $($p.Id))"
    Stop-Process -Id $p.Id -Force
    Write-Output "Stopped"
} else {
    Write-Output "EXITED with code: $($p.ExitCode)"
}
