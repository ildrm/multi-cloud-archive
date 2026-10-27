param([int]$Port = 9224, [Parameter(Mandatory=$true)][string]$Executable)
$ErrorActionPreference = 'Stop'
if ($Port -le 1024 -or $Port -gt 65535) { throw 'Use an unprivileged local inspection port.' }
$env:CLOUDWEAVE_SMOKE_TEST = '1'
$env:CLOUDWEAVE_INSPECT_PORT = $Port.ToString()
try { & $Executable } finally {
    Remove-Item Env:CLOUDWEAVE_SMOKE_TEST -ErrorAction SilentlyContinue
    Remove-Item Env:CLOUDWEAVE_INSPECT_PORT -ErrorAction SilentlyContinue
}
