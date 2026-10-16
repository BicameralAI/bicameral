# Design System — Bicameral Dashboard

## Product Context
- **What this is:** The operator dashboard for Bicameral — a decision ledger that binds team decisions to code evidence, verified by a local daemon and rendered by a hosted cloud view.
- **Who it's for:** Engineering teams (operators, EMs, ICs) governing decisions across code, docs, and channels.
- **Space/industry:** Developer governance / engineering intelligence tooling.
- **Project type:** Data-dense web dashboard (Svelte 5 + Vite, `dashboard/`).

## The Memorable Thing
"An instrument panel for decisions." The UI should feel like verified machinery — a ledger you trust — not a SaaS brochure.

## Aesthetic Direction
- **Direction:** Industrial/Utilitarian ledger. Light canvas, hard grid, hairline borders, dense tables, colored status pills.
- **Decoration level:** Minimal. Typography, spacing, and the two accent hues do all the work. No gradients, no illustration, no decorative blobs.
- **Mood:** Calm authority. Every colored element carries meaning; nothing is colored for style.

## Voice & Verbosity (hard rule)
- **Terse by default.** Labels over sentences. Values over prose.
- **Never auto-generate "warning" / "information" / explainer sections, banners, or footer strips.** If a state needs explanation, put one short line inline next to the affected element — nothing else.
- Empty states get one line + one action, not a paragraph.
- Override only when the user explicitly asks for explanatory copy.

## Logo
- Mark: the broken circle (two arcs with diagonal gaps), from `../bicameral/site/static/logo.png`.
- In-app: inline SVG (see `App.svelte` `.logo-mark`) — ink-900 square tile, white arcs, 4px radius. Wordmark `BICAMERAL` in 700 weight, 0.08em tracking.

## Typography
- **UI/Body:** IBM Plex Sans — enterprise-ledger feel, excellent at 13px data sizes, tabular numerals available.
- **Machine truth (IDs, SHAs, paths, timestamps, counts):** JetBrains Mono — brand continuity with the marketing site. Monospace is a first-class citizen: anything the daemon computed renders in mono.
- **Loading:** Google Fonts, weights 400/500/600/700 (Plex), 400/500/600 (Mono).
- **Scale:** 11px meta/pills · 13px controls/tables · 14px body · 16px section headings · 20px page titles · 24px hero numbers.

## Color
- **Approach:** Restrained two-accent system. The split encodes the product's bicameral concept:
  - **Teal `--accent-*` (#0891b2):** operational authority — daemon-verified, bound, fresh, healthy connection. Primary actions.
  - **Purple `--memory-*` (#8b5cf6):** memory/graph/advisory — needs review, partial binding, projections, graph nodes.
- **Neutrals:** canvas `#fcfcfd`, surface `#fff`, ink `#18181b → #a1a1aa`, hairlines `#e4e4e7`.
- **Semantic:** green `#10b981` healthy/fresh · amber `#f59e0b` stale/advisory · red `#ef4444` blocked/needs signoff.
- **Rule:** never introduce a new hue. If a state doesn't map to teal/purple/green/amber/red/neutral, it's neutral.
- **Dark mode:** not implemented; when added, invert surfaces (ink canvas, zinc-900 surface) and desaturate accents ~15%.

## Spacing
- **Base unit:** 4px. **Density:** compact.
- **Scale (CSS vars):** `--sp-1`(4) `--sp-2`(8) `--sp-3`(12) `--sp-4`(16) `--sp-5`(20) `--sp-6`(24) `--sp-8`(32).

## Layout
- **Approach:** grid-disciplined. Top bar (brand · context · session) → sidebar nav (icon + label, active = teal left bar) → main content; optional right detail panel for selected records.
- **Borders over shadows:** 1px `--line` hairlines define structure; shadows (`--shadow-1/2`) only for overlays.
- **Border radius:** sm 4 · md 6 · lg 8 · pill 999. Tables and page chrome stay square-ish; only pills are fully round.
- **Hosted ops shell density** (Products + Settings): follow Langfuse / shadcn sidebar formula via CSS vars in `app.css`:
  - `--shell-sidebar-w` ~13rem, `--shell-topbar-h` 40px, `--shell-nav-item-h` 28px
  - **No page-level essay headers** (no eyebrow + H1 + marketing blurb). Title lives only in the thin topbar; sections use a single compact title row.
  - External formula refs: [shadcn Sidebar](https://ui.shadcn.com/docs/components/sidebar), Langfuse app shell (Tailwind + shadcn/ui).

## Motion
- **Approach:** minimal-functional. Hover/state transitions only.
- **Easing:** `--ease-out` cubic-bezier(0.16,1,0.3,1). **Duration:** 120ms hover, 200ms panel.

## Implementation
- Tokens + shared primitives live in `dashboard/src/app.css` (imported in `main.ts`). Components must use `var(--*)` tokens; no hard-coded hex, font stacks, or magic px values in component `<style>` blocks.
- Shared classes: `.pill .pill-{teal,purple,green,amber,red,neutral}`, `.dot .dot-*`, `.card`, `.data-table`, `.btn .btn-primary`, `.mono`.

## Decisions Log
| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-07-07 | Initial design system | Built from ledger-style reference mockup; reuses marketing-site token values (teal accent, purple memory scales) |
| 2026-07-07 | Anti-verbosity rule | User directive: no auto-generated warning/info sections anywhere |
| 2026-07-07 | IBM Plex Sans replaces Inter | Enterprise-ledger feel matching reference; Inter is the convergence default |
