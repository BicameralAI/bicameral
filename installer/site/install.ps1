[CmdletBinding()]
param(
    [switch]$Uninstall,
    [ValidatePattern('^[a-f0-9]{40}$')]
    [string]$ExpectedBotCommit,
    [ValidatePattern('^[a-f0-9]{64}$')]
    [string]$ExpectedWindowsSha256
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$DownloadBase = if ($env:BICAMERAL_DOWNLOAD_BASE) {
    $env:BICAMERAL_DOWNLOAD_BASE.TrimEnd('/')
} else {
    'https://pub-7eab94b264754bee8019fd89ca9b5b0b.r2.dev/bicameral'
}
$InstallDir = if ($env:BICAMERAL_INSTALL_DIR) {
    $env:BICAMERAL_INSTALL_DIR
} else {
    Join-Path $env:LOCALAPPDATA 'Bicameral\bin'
}
$TaskName = if ($env:BICAMERAL_TASK_NAME) {
    $env:BICAMERAL_TASK_NAME
} else {
    'Bicameral'
}
$Target = 'x86_64-pc-windows-msvc'
$GatewayPort = if ($env:BICAMERAL_GATEWAY_PORT) { [int]$env:BICAMERAL_GATEWAY_PORT } else { 37373 }
$Destination = Join-Path $InstallDir 'bicameral.exe'

function Write-Step([string]$Message) {
    Write-Host "[Bicameral] $Message"
}

function Invoke-Checked([string]$FilePath, [string[]]$Arguments) {
    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$FilePath failed with exit code $LASTEXITCODE"
    }
}

function Get-BicameralTask {
    return Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
}

function Test-BicameralTask {
    return $null -ne (Get-BicameralTask)
}

function Stop-BicameralProcess {
    # Task Scheduler owns a PowerShell wrapper, so stopping the task does not
    # guarantee that its bicameral.exe child exits. Kill only the executable
    # installed at this exact destination; never terminate another Bicameral
    # process from a different installation.
    $Running = @(
        Get-Process -Name 'bicameral' -ErrorAction SilentlyContinue |
            Where-Object { $_.Path -and ($_.Path -ieq $Destination) }
    )
    foreach ($Process in $Running) {
        Stop-Process -Id $Process.Id -Force -ErrorAction SilentlyContinue
        try {
            $Process.WaitForExit(5000) | Out-Null
        } catch {
            # Replacement/removal below remains fail-closed if Windows still
            # holds the executable after this bounded best-effort shutdown.
        }
    }
}

function Stop-BicameralTask {
    if (-not (Test-BicameralTask)) {
        Stop-BicameralProcess
        return $false
    }
    Write-Step 'Stopping the existing Bicameral background task...'
    Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
    Stop-BicameralProcess
    Start-Sleep -Milliseconds 500
    return $true
}

function Remove-BicameralTask {
    if (Test-BicameralTask) {
        Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
        Stop-BicameralProcess
        Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false
    } else {
        Stop-BicameralProcess
    }
}

function Register-BicameralTask {
    $Identity = [Security.Principal.WindowsIdentity]::GetCurrent().Name
    if (-not $Identity) {
        throw 'Could not resolve the current Windows user for Scheduled Task registration.'
    }

    $WindowsPowerShell = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    if (-not (Test-Path -LiteralPath $WindowsPowerShell -PathType Leaf)) {
        throw 'Windows PowerShell 5.1 is required to host the Bicameral background task.'
    }
    $EscapedDestination = $Destination.Replace("'", "''")
    $TaskArguments = "-NoLogo -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -Command `"& '$EscapedDestination' gateway start`""

    $Action = New-ScheduledTaskAction -Execute $WindowsPowerShell -Argument $TaskArguments
    $Trigger = New-ScheduledTaskTrigger -AtLogOn -User $Identity
    $Principal = New-ScheduledTaskPrincipal -UserId $Identity -LogonType Interactive -RunLevel Limited
    $Settings = New-ScheduledTaskSettingsSet `
        -AllowStartIfOnBatteries `
        -DontStopIfGoingOnBatteries `
        -StartWhenAvailable `
        -MultipleInstances IgnoreNew `
        -ExecutionTimeLimit ([TimeSpan]::Zero)

    Register-ScheduledTask `
        -TaskName $TaskName `
        -Action $Action `
        -Trigger $Trigger `
        -Principal $Principal `
        -Settings $Settings `
        -Force | Out-Null
}

function Start-BicameralTask {
    Start-ScheduledTask -TaskName $TaskName
}

function Install-BinaryWithRetry([string]$Source, [string]$DestinationPath) {
    $LastError = $null
    for ($Attempt = 0; $Attempt -lt 20; $Attempt++) {
        try {
            Copy-Item -LiteralPath $Source -Destination $DestinationPath -Force
            return
        } catch {
            $LastError = $_
            Start-Sleep -Milliseconds 250
        }
    }
    throw "Could not replace the Bicameral executable after waiting for the prior process to exit: $($LastError.Exception.Message)"
}

function Remove-InstallDirFromUserPath {
    $UserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not $UserPath) { return }
    $Remaining = @($UserPath -split ';' | Where-Object { $_ -and $_.TrimEnd('\\') -ne $InstallDir.TrimEnd('\\') })
    [Environment]::SetEnvironmentVariable('Path', ($Remaining -join ';'), 'User')
}

function Get-RequiredProperty([object]$Object, [string]$Name, [string]$Message) {
    if ($null -eq $Object) { throw $Message }
    $Property = $Object.PSObject.Properties[$Name]
    if ($null -eq $Property -or $null -eq $Property.Value) { throw $Message }
    return $Property.Value
}

if (-not [Environment]::Is64BitOperatingSystem) { throw 'Bicameral APC1 currently supports Windows x64 only.' }

if ($Uninstall) {
    Write-Step 'Removing Bicameral from this user account...'
    Remove-BicameralTask
    if (Test-Path -LiteralPath $Destination) { Remove-Item -LiteralPath $Destination -Force }
    if (Test-Path -LiteralPath $InstallDir) {
        $RemainingFiles = @(Get-ChildItem -LiteralPath $InstallDir -Force -ErrorAction SilentlyContinue)
        if ($RemainingFiles.Count -eq 0) { Remove-Item -LiteralPath $InstallDir -Force }
    }
    Remove-InstallDirFromUserPath
    Write-Step 'Bicameral was removed. Open a new terminal for the PATH change to take effect.'
    return
}

if ($PSBoundParameters.ContainsKey('ExpectedBotCommit') -ne $PSBoundParameters.ContainsKey('ExpectedWindowsSha256')) {
    throw 'ExpectedBotCommit and ExpectedWindowsSha256 must be supplied together.'
}

$ManifestUrl = "$DownloadBase/latest.json"
$ManifestUri = [Uri]$ManifestUrl
if (-not $ManifestUri.IsAbsoluteUri -or $ManifestUri.Scheme -ne 'https') { throw 'Bicameral release manifest URL must use HTTPS.' }
Write-Step 'Reading governed release manifest...'
$Manifest = Invoke-RestMethod -Uri $ManifestUri.AbsoluteUri -UseBasicParsing
$Assets = Get-RequiredProperty $Manifest 'assets' 'The governed Bicameral release manifest is missing its assets map.'
$AssetProperty = $Assets.PSObject.Properties[$Target]
if ($null -eq $AssetProperty -or $null -eq $AssetProperty.Value) { throw 'The current Bicameral release does not publish the required Windows x64 asset yet.' }
$Asset = $AssetProperty.Value
$AssetUrl = [string](Get-RequiredProperty $Asset 'url' 'Windows release asset is missing its download URL.')
$AssetSha256 = [string](Get-RequiredProperty $Asset 'sha256' 'Windows release asset is missing its SHA-256 digest.')
$AssetUri = [Uri]$AssetUrl
if (-not $AssetUri.IsAbsoluteUri -or $AssetUri.Scheme -ne 'https') { throw 'Windows release asset URL must use HTTPS.' }
if ($AssetSha256 -notmatch '^[a-f0-9]{64}$') { throw 'Windows release asset is missing a valid SHA-256 digest.' }
# Pin at the consumption point, before downloading or stopping an existing daemon.
# A prior manifest check alone cannot prevent a later latest.json publication.
if ($PSBoundParameters.ContainsKey('ExpectedBotCommit')) {
    $PublishedCommit = [string](Get-RequiredProperty $Manifest 'bot_commit' 'The governed release manifest is missing its Bot commit.')
    if ($PublishedCommit -cne $ExpectedBotCommit) {
        throw "Published Bot commit $PublishedCommit does not match expected $ExpectedBotCommit."
    }
    if ($AssetSha256 -cne $ExpectedWindowsSha256) {
        throw "Published Windows SHA-256 $AssetSha256 does not match expected $ExpectedWindowsSha256."
    }
}
$ManifestVersionProperty = $Manifest.PSObject.Properties['version']
$ManifestVersion = if ($null -ne $ManifestVersionProperty -and $ManifestVersionProperty.Value) { [string]$ManifestVersionProperty.Value } else { 'current' }

$TempRoot = Join-Path ([IO.Path]::GetTempPath()) ("bicameral-install-" + [guid]::NewGuid().ToString('N'))
$Archive = Join-Path $TempRoot 'bicameral-windows.tar.gz'
$ExtractDir = Join-Path $TempRoot 'extract'
New-Item -ItemType Directory -Path $ExtractDir -Force | Out-Null
$ExistingTaskWasPresent = $false
$InstallSucceeded = $false

try {
    Write-Step "Downloading Bicameral $ManifestVersion for Windows x64..."
    Invoke-WebRequest -Uri $AssetUri.AbsoluteUri -OutFile $Archive -UseBasicParsing
    Write-Step 'Verifying release integrity...'
    $ActualHash = (Get-FileHash -Path $Archive -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($ActualHash -ne $AssetSha256.ToLowerInvariant()) { throw "Checksum mismatch. Expected $AssetSha256, got $ActualHash." }
    Write-Step 'Extracting verified release...'
    Invoke-Checked 'tar.exe' @('-xzf', $Archive, '-C', $ExtractDir)
    $Binary = Join-Path $ExtractDir 'bicameral.exe'
    if (-not (Test-Path -LiteralPath $Binary -PathType Leaf)) { throw 'Verified archive did not contain bicameral.exe.' }

    $ExistingTaskWasPresent = Stop-BicameralTask
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    Install-BinaryWithRetry -Source $Binary -DestinationPath $Destination
    Write-Step "Installed Bicameral to $Destination"

    $UserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $PathEntries = @($UserPath -split ';' | Where-Object { $_ })
    if ($PathEntries -notcontains $InstallDir) {
        $NewUserPath = if ($UserPath) { "$UserPath;$InstallDir" } else { $InstallDir }
        [Environment]::SetEnvironmentVariable('Path', $NewUserPath, 'User')
    }
    if (($env:Path -split ';') -notcontains $InstallDir) { $env:Path = "$InstallDir;$env:Path" }

    Write-Step 'Registering the per-user Bicameral background task...'
    Register-BicameralTask
    Write-Step 'Starting Bicameral...'
    Start-BicameralTask

    $Ready = $false
    for ($Attempt = 0; $Attempt -lt 20; $Attempt++) {
        Start-Sleep -Milliseconds 500
        try {
            $Client = [Net.Sockets.TcpClient]::new()
            $Connect = $Client.ConnectAsync('127.0.0.1', $GatewayPort)
            if ($Connect.Wait(500) -and $Client.Connected) { $Ready = $true; $Client.Dispose(); break }
            $Client.Dispose()
        } catch {}
    }
    if ($Ready) {
        Write-Step "Bicameral is running on localhost:$GatewayPort. Return to the browser to continue pairing."
    } else {
        Write-Warning "Bicameral was installed, but localhost:$GatewayPort is not reachable yet. Return to the browser and use Try again; if it still fails, run '$Destination doctor'."
    }
    $InstallSucceeded = $true
    Write-Host ''
    Write-Host 'Installation complete.'
    Write-Host 'The Bicameral browser page can now continue automatically when the daemon is ready.'
    Write-Host ''
    Write-Host "Repair/start: Start-ScheduledTask -TaskName '$TaskName'"
    Write-Host 'Uninstall (paste into PowerShell): $p = Join-Path $env:TEMP (''bicameral-uninstall-'' + [guid]::NewGuid().ToString(''N'') + ''.ps1''); try { Invoke-WebRequest -Uri ''https://install.bicameral-ai.com/install.ps1'' -OutFile $p -UseBasicParsing -ErrorAction Stop; & $p -Uninstall } finally { Remove-Item -LiteralPath $p -Force -ErrorAction SilentlyContinue }'
} catch {
    if (-not $InstallSucceeded -and $ExistingTaskWasPresent -and (Test-Path -LiteralPath $Destination)) {
        Start-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
    }
    throw
} finally {
    if (Test-Path -LiteralPath $TempRoot) { Remove-Item -LiteralPath $TempRoot -Recurse -Force -ErrorAction SilentlyContinue }
}
