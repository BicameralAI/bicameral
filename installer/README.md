# Bicameral Install

Static installer site for Bicameral Bot.

Primary install command:

```bash
curl -fsSL https://install.bicameral-ai.com/install.sh | sh
bicameral dashboard
```

The live site at `install.bicameral-ai.com` is still deployed from the original `bicameral-install`
repository; this directory is its source, and nothing is deployed from here. Release binaries are
published by `bicameral-bot` to Cloudflare R2 and
downloaded through `https://pub-7eab94b264754bee8019fd89ca9b5b0b.r2.dev/bicameral`.

## Tests

```bash
sh -n site/install.sh && ./tests/test-unix-reconcile.sh   # Unix installer
pwsh -NoProfile -File tests/test-windows-publication.ps1   # Windows installer (PowerShell 7)
```

`site/install.ps1` and `site/install-windows.cmd` are the Windows installer and its consent launcher. CI runs the
Windows checks on `windows-latest`; the installed lifecycle on Windows PowerShell 5.1 runs only on manual dispatch
against a published release and is not verified from this repository.
