param(
  [Parameter(Mandatory = $true)][string]$Target,
  [string]$PackageName = 'frigg'
)

$ErrorActionPreference = 'Stop'

cargo build --locked --release -p $PackageName --target $Target
