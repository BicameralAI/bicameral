# External brand marks

Canonical folder for third-party logos used in the dashboard Settings / connection rows.

| File | Brand | Source | License |
|------|--------|--------|---------|
| `googledrive.svg` | Google Drive | Multi-color Drive triangle (Google brand colors) | Google trademarks; internal product UI only |
| `claude.svg` | Claude | [Simple Icons](https://simpleicons.org/) `claude` @ 15.18.0, fill `#D97757` | [CC0-1.0](https://github.com/simple-icons/simple-icons/blob/develop/LICENSE.md) |
| `openai.svg` | OpenAI | Simple Icons `openai` @ 15.18.0 | CC0-1.0 |
| `codex.svg` | Codex | Same mark as OpenAI (Codex product) | CC0-1.0 |
| `anthropic.svg` | Anthropic | Simple Icons `anthropic` @ 15.18.0 | CC0-1.0 |

**Do not hotlink CDNs in production UI.** Vendored SVGs stay in-repo so builds are offline-reproducible.

Refresh from Simple Icons (monochrome brands):

```bash
VER=15.18.0
curl -fsSL "https://cdn.jsdelivr.net/npm/simple-icons@${VER}/icons/claude.svg" -o claude.svg
# then re-apply brand fill hex as above
```
