# Bicameral Install

Static installer site for Bicameral Bot.

Primary install command:

```bash
curl -fsSL https://install.bicameral-ai.com/install.sh | sh
bicameral dashboard
```

This repository is deployed with GitHub Pages at `install.bicameral-ai.com`.
Release binaries are published by `bicameral-bot` to Cloudflare R2 and
downloaded through `https://pub-7eab94b264754bee8019fd89ca9b5b0b.r2.dev/bicameral`.
