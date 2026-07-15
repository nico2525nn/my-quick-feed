$url = "https://aka.ms/vs/17/release/vs_BuildTools.exe"
$out = "C:\vs_BuildTools.exe"
if (!(Test-Path $out)) {
    Write-Output "Downloading VS BuildTools..."
    Invoke-WebRequest -Uri $url -OutFile $out
}
Write-Output "Installing VC++ workload..."
$p = Start-Process -Wait -PassThru -FilePath $out -ArgumentList "--quiet", "--norestart", "--add", "Microsoft.VisualStudio.Workload.VCTools", "--includeRecommended"
Write-Output "Exit code: $($p.ExitCode)"
