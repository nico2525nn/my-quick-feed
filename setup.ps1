$tmp = $env:TEMP
Write-Output "Downloading rustup-init.exe..."
Invoke-WebRequest -Uri https://win.rustup.rs -OutFile "$tmp\rustup-init.exe"
Write-Output "Running rustup-init.exe..."
Start-Process -Wait -FilePath "$tmp\rustup-init.exe" -ArgumentList "-y", "--default-toolchain", "stable"
Write-Output "Done"
