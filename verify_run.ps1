Write-Output "=== Testing run.bat ==="
$p = Start-Process -Wait -NoNewWindow -PassThru -FilePath "C:\Windows\System32\cmd.exe" -ArgumentList "/c", "C:\quickfeed\run.bat"
Write-Output "Exit code: $($p.ExitCode)"
Write-Output "=== Done ==="
