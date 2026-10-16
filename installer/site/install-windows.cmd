@echo off
setlocal
echo Bicameral Windows installer
echo.
echo This will download the governed Bicameral installer over HTTPS, verify the
echo released binary checksum, install it for your user account, and start the
echo local Bicameral background task.
echo.
choice /C YN /N /M "Continue? [Y/N] "
if errorlevel 2 exit /b 1

powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -Command "$ErrorActionPreference='Stop'; $p=Join-Path $env:TEMP ('bicameral-install-' + [guid]::NewGuid().ToString('N') + '.ps1'); try { Invoke-WebRequest -Uri 'https://install.bicameral-ai.com/install.ps1' -OutFile $p -UseBasicParsing; & $p } finally { Remove-Item -LiteralPath $p -Force -ErrorAction SilentlyContinue }"
if errorlevel 1 (
  echo.
  echo Bicameral installation failed. Review the message above before retrying.
  pause
  exit /b 1
)

echo.
echo Bicameral installation finished. Return to your browser to continue.
pause
