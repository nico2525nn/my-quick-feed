$path = "$env:APPDATA\com.myquickfeed.app\my-quick-feed.yaml"
Write-Output "Checking: $path"
if (Test-Path $path) {
    Write-Output "=== EXISTS ==="
    Get-Content $path
    Write-Output "=== END ==="
} else {
    Write-Output "NOT FOUND"
}
