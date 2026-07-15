$setup = "C:\Program Files (x86)\Microsoft Visual Studio\Installer\setup.exe"
$installPath = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools"
$args = @(
    "modify",
    "--installPath", $installPath,
    "--add", "Microsoft.VisualStudio.Workload.VCTools",
    "--includeRecommended",
    "--quiet",
    "--norestart"
)
Write-Output "Running: $setup $($args -join ' ')"
$proc = Start-Process -Wait -PassThru -FilePath $setup -ArgumentList $args
Write-Output "Exit code: $($proc.ExitCode)"
