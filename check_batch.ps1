$proc = Start-Process -NoNewWindow -PassThru -FilePath "cmd" -ArgumentList "/c", "C:\quickfeed\run.bat"
Start-Sleep 5
if (-not $proc.HasExited) {
    Write-Output "Binary is running - OK"
    $proc.Kill()
    Write-Output "Test process killed"
} else {
    Write-Output "Process exited with code: $($proc.ExitCode)"
}
