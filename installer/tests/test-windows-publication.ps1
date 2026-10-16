$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$Installer = Join-Path $PSScriptRoot '../site/install.ps1'
$TestRoot = Join-Path ([IO.Path]::GetTempPath()) ('bicameral-publication-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $TestRoot | Out-Null
$PreviousInstallDir = $env:BICAMERAL_INSTALL_DIR
$PreviousLocal = $env:LOCALAPPDATA
$PreviousTemp = $env:TEMP
$env:BICAMERAL_INSTALL_DIR = Join-Path $TestRoot 'install'
$env:LOCALAPPDATA = $TestRoot
$env:TEMP = $TestRoot
$CommitA = 'a' * 40
$CommitB = 'b' * 40
$DigestA = 'a' * 64
$DigestB = 'b' * 64
$global:BicameralTestDownloads = 0
$global:BicameralTestMockManifest = $null
function Invoke-RestMethod { param($Uri, [switch]$UseBasicParsing) return $global:BicameralTestMockManifest }
function Invoke-WebRequest {
    param($Uri, $OutFile, [switch]$UseBasicParsing)
    $global:BicameralTestDownloads++
    throw 'TEST_DOWNLOAD_REACHED'
}
function New-Manifest($Commit, $Digest) {
    return [pscustomobject]@{
        bot_commit = $Commit; version = 'test'
        assets = [pscustomobject]@{
            'x86_64-pc-windows-msvc' = [pscustomobject]@{
                url = 'https://example.invalid/bicameral.tar.gz'; sha256 = $Digest
            }
        }
    }
}
function Check-Consumption($Name, $Manifest, $Pins, $ExpectedError, $ExpectedDownloads) {
    $global:BicameralTestMockManifest = $Manifest
    $global:BicameralTestDownloads = 0
    $Caught = ''
    try { & $Installer @Pins } catch { $Caught = $_.Exception.Message }
    if (-not $Caught.Contains($ExpectedError)) { throw "$Name returned unexpected error: $Caught" }
    if ($global:BicameralTestDownloads -ne $ExpectedDownloads) { throw "$Name reached download $global:BicameralTestDownloads times, expected $ExpectedDownloads." }
    if (Test-Path -LiteralPath $env:BICAMERAL_INSTALL_DIR) { throw "$Name changed the installation." }
    Write-Host "PASS: $Name"
}
try {
    $Pins = @{ ExpectedBotCommit = $CommitA; ExpectedWindowsSha256 = $DigestA }
    Check-Consumption 'checked publication accepted' (New-Manifest $CommitA $DigestA) $Pins 'TEST_DOWNLOAD_REACHED' 1
    # The precheck has accepted A. A later consumption sees B and must refuse.
    foreach ($Point in @('install', 'rerun')) {
        Check-Consumption "$Point rejects changed commit" (New-Manifest $CommitB $DigestA) $Pins 'does not match expected' 0
        Check-Consumption "$Point rejects changed digest" (New-Manifest $CommitA $DigestB) $Pins 'does not match expected' 0
    }
    $Missing = New-Manifest $CommitA $DigestA
    $Missing.PSObject.Properties.Remove('bot_commit')
    Check-Consumption 'missing commit refused when pinned' $Missing $Pins 'missing its Bot commit' 0
    Check-Consumption 'commit-only pin refused' (New-Manifest $CommitA $DigestA) @{ExpectedBotCommit=$CommitA} 'must be supplied together' 0
    Check-Consumption 'digest-only pin refused' (New-Manifest $CommitA $DigestA) @{ExpectedWindowsSha256=$DigestA} 'must be supplied together' 0
    Check-Consumption 'ordinary customer latest remains supported' (New-Manifest $CommitB $DigestB) @{} 'TEST_DOWNLOAD_REACHED' 1

    # Execute the actual printed uninstall command from an unrelated directory.
    $Tokens = $null; $ParseErrors = $null
    $Ast = [Management.Automation.Language.Parser]::ParseFile($Installer, [ref]$Tokens, [ref]$ParseErrors)
    if ($ParseErrors.Count) { throw 'Installer parse failed.' }
    $Guidance = @($Ast.FindAll({
        param($Node)
        $Node -is [Management.Automation.Language.StringConstantExpressionAst] -and
        $Node.Value.StartsWith('Uninstall (paste into PowerShell): ')
    }, $true))
    if ($Guidance.Count -ne 1) { throw 'Expected one runnable browser uninstall instruction.' }
    $Command = $Guidance[0].Value.Substring('Uninstall (paste into PowerShell): '.Length)
    $global:BicameralTestFetchedUninstall = $null
    function Invoke-WebRequest {
        param($Uri, $OutFile, [switch]$UseBasicParsing, $ErrorAction)
        if ($Uri -ne 'https://install.bicameral-ai.com/install.ps1') { throw 'Wrong uninstall origin.' }
        $global:BicameralTestFetchedUninstall = $OutFile
        Set-Content -LiteralPath $OutFile -Value 'param([switch]$Uninstall); if (-not $Uninstall) { throw "Uninstall switch missing" }; $global:BicameralTestGuidanceInvoked = $true'
    }
    $global:BicameralTestGuidanceInvoked = $false
    Push-Location $TestRoot
    try { & ([ScriptBlock]::Create($Command)) } finally { Pop-Location }
    if (-not $global:BicameralTestGuidanceInvoked) { throw 'Printed command did not invoke the fetched uninstaller.' }
    if (Test-Path -LiteralPath $global:BicameralTestFetchedUninstall) { throw 'Printed command leaked its temporary script.' }
    Write-Host 'PASS: browser uninstall guidance works outside checkout and cleans temporary script'

    # Download failure must not execute an old script and must still clean up.
    $global:BicameralTestGuidanceInvoked = $false
    function Invoke-WebRequest {
        param($Uri, $OutFile, [switch]$UseBasicParsing, $ErrorAction)
        $global:BicameralTestFetchedUninstall = $OutFile
        Set-Content -LiteralPath $OutFile -Value 'throw "Partial download executed"'
        throw 'TEST_UNINSTALL_DOWNLOAD_FAILED'
    }
    $Caught = ''
    try { & ([ScriptBlock]::Create($Command)) } catch { $Caught = $_.Exception.Message }
    if (-not $Caught.Contains('TEST_UNINSTALL_DOWNLOAD_FAILED')) { throw "Download failure was masked: $Caught" }
    if (Test-Path -LiteralPath $global:BicameralTestFetchedUninstall) { throw 'Failed download leaked temporary script.' }
    Write-Host 'PASS: uninstall download failure stops execution and cleans temporary script'
} finally {
    Remove-Variable -Name BicameralTestDownloads,BicameralTestMockManifest,BicameralTestFetchedUninstall,BicameralTestGuidanceInvoked -Scope Global -ErrorAction SilentlyContinue
    $env:BICAMERAL_INSTALL_DIR = $PreviousInstallDir
    $env:LOCALAPPDATA = $PreviousLocal
    $env:TEMP = $PreviousTemp
    Remove-Item -LiteralPath $TestRoot -Recurse -Force -ErrorAction SilentlyContinue
}
