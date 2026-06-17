param(
  [Parameter(Mandatory = $true)][string]$Target,
  [string]$Version = '',
  [string]$BinName = 'frigg',
  [string]$OutDir = 'dist'
)

$ErrorActionPreference = 'Stop'

if ($Version) {
  $archivePath = Join-Path $OutDir "$BinName-v$Version-$Target.zip"
} else {
  $archives = @(Get-ChildItem -Path $OutDir -Filter "$BinName-v*-$Target.zip" -File)
  if ($archives.Count -ne 1) {
    throw "Expected exactly one archive for target $Target, found $($archives.Count). Set Version to disambiguate."
  }
  $archivePath = $archives[0].FullName
}

if (-not (Test-Path $archivePath)) {
  throw "Archive not found: $archivePath"
}

$checksumPath = "$archivePath.sha256"
if (-not (Test-Path $checksumPath)) {
  throw "Checksum not found: $checksumPath"
}

$expected = ((Get-Content -Path $checksumPath -Raw) -split '\s+')[0]
$actual = (Get-FileHash -Algorithm SHA256 -Path $archivePath).Hash
if ($actual.ToLowerInvariant() -ne $expected.ToLowerInvariant()) {
  throw "Checksum mismatch for $archivePath"
}

$smokeDir = Join-Path ([System.IO.Path]::GetTempPath()) ("frigg-smoke-" + [Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Force -Path $smokeDir | Out-Null
try {
  Expand-Archive -Path $archivePath -DestinationPath $smokeDir -Force
  $binPath = Join-Path $smokeDir "$BinName.exe"
  if (-not (Test-Path $binPath)) {
    throw "Binary not found in archive: $binPath"
  }

  & $binPath --version | Out-Null
  & $binPath --help | Out-Null
  & $binPath serve --help | Out-Null
} finally {
  Remove-Item -Recurse -Force $smokeDir
}
