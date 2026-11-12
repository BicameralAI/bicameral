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

    # Execute the actual printed uninstall command the way a customer does: pasted
    # into a fresh session, from an unrelated directory, with the Windows client
    # default execution policy (Restricted). The download is replaced by a stub
    # script that records the call and exits with a chosen code.
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
    $HostExe = (Get-Process -Id $PID).Path
    $Marker = Join-Path $TestRoot 'uninstall-invoked.txt'
    $env:BICAMERAL_TEST_MARKER = $Marker
    $env:BICAMERAL_TEST_STUB = "param([switch]`$Uninstall)`nif (-not `$Uninstall) { exit 9 }`nSet-Content -LiteralPath `$env:BICAMERAL_TEST_MARKER -Value invoked`nexit ([int]`$env:BICAMERAL_TEST_EXIT)"
    $ChildPrelude = @'
if ((Get-ExecutionPolicy) -ne 'Restricted') { exit 42 }
function Invoke-WebRequest {
    param($Uri, $OutFile, [switch]$UseBasicParsing, $ErrorAction)
    if ($Uri -ne 'https://install.bicameral-ai.com/install.ps1') { throw 'Wrong uninstall origin.' }
    Set-Content -LiteralPath $OutFile -Value $env:BICAMERAL_TEST_STUB
}
'@
    function Invoke-PrintedUninstall([int]$StubExitCode) {
        $ErrorActionPreference = 'Continue'
        $env:BICAMERAL_TEST_EXIT = [string]$StubExitCode
        if (Test-Path -LiteralPath $Marker) { Remove-Item -LiteralPath $Marker -Force }
        $Encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($ChildPrelude + "`n" + $Command))
        Push-Location $TestRoot
        try {
            $Output = & $HostExe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Restricted -EncodedCommand $Encoded 2>&1 | Out-String
            $Code = $LASTEXITCODE
        } finally { Pop-Location }
        if ($Code -eq 42) { throw 'Test setup could not create a process with a Restricted execution policy.' }
        if (@(Get-ChildItem -LiteralPath $TestRoot -Filter 'bicameral-uninstall-*.ps1' -Force).Count) { throw 'Printed command leaked its temporary script.' }
        return [pscustomobject]@{ Code = $Code; Output = $Output; Invoked = (Test-Path -LiteralPath $Marker) }
    }

    $Result = Invoke-PrintedUninstall 0
    if (-not $Result.Invoked -or $Result.Code -ne 0) { throw "Printed command did not run the fetched uninstaller under a Restricted policy (exit $($Result.Code)): $($Result.Output)" }
    Write-Host 'PASS: browser uninstall guidance works under a Restricted execution policy, outside checkout, and cleans temporary script'

    $Result = Invoke-PrintedUninstall 3
    if (-not $Result.Invoked) { throw "Printed command did not run the fetched uninstaller (exit $($Result.Code)): $($Result.Output)" }
    if ($Result.Code -eq 0 -or -not $Result.Output.Contains('exit code 3')) { throw "Printed command hid a failed uninstall (exit $($Result.Code)): $($Result.Output)" }
    Write-Host 'PASS: browser uninstall guidance reports a failed uninstaller and cleans temporary script'

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
    # The failed-uninstaller case leaves a non-zero native exit code, and the
    # CI step wrapper exits with $LASTEXITCODE. All checks passed at this point.
    $global:LASTEXITCODE = 0
} finally {
    Remove-Variable -Name BicameralTestDownloads,BicameralTestMockManifest,BicameralTestFetchedUninstall,BicameralTestGuidanceInvoked -Scope Global -ErrorAction SilentlyContinue
    Remove-Item -Path Env:BICAMERAL_TEST_MARKER,Env:BICAMERAL_TEST_STUB,Env:BICAMERAL_TEST_EXIT -ErrorAction SilentlyContinue
    $env:BICAMERAL_INSTALL_DIR = $PreviousInstallDir
    $env:LOCALAPPDATA = $PreviousLocal
    $env:TEMP = $PreviousTemp
    Remove-Item -LiteralPath $TestRoot -Recurse -Force -ErrorAction SilentlyContinue
}
