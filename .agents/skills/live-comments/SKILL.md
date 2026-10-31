---
name: live-comments
description: Annotate business rules with @source and @open live comments. Use when writing or changing code in a project that has live-comments.config.json, especially in its governed areas, or when the user asks where a rule came from or who should decide it.
---

# Live Comments

This project marks where its business rules come from. A `@source` cites the ticket, document or file a rule rests on. An `@open` marks a rule that nobody has decided yet. A checker verifies both. Your job is to annotate the rules you write the way the rest of the codebase does.

## Setting up a project

If the project has no `live-comments.config.json`, or the user asks you to set up live-comments:

1. Run `npx live-comments init`. It creates an empty config and `AUTHORITY` file, installs this skill and registers the MCP server. It makes no choices for the project.
2. **Governed areas.** Look through the codebase for where business rules live (pricing, billing, eligibility, permissions, policy) and propose `governed` globs to the user. Start narrow; it is easy to add areas later.
3. **Sources.** Ask the user which tools their decisions live in, and have them run `npx live-comments connect <system>` for each one.
4. **Authority.** Draft person definitions from `npx live-comments directory <system>` and `git log` authors, then groups and places from the teams the user describes. Don't merge two accounts into one person unless an email matches or the user confirms it. Show the full draft to the user before writing it.
5. Add the CI step `init` printed, and run `npx live-comments check`.

## Before you write a business rule

Run:

```
npx live-comments context <file>:<line>
```

It tells you whether the line is in a governed area, which annotations are nearby, which authorities they use (`by …`), and whether `sticky` is on. Read it before deciding anything below.

## Decide what the rule needs

A business rule is a choice about pricing, eligibility, limits, timing, rounding, permissions or policy that someone outside engineering could disagree with. Implementation details (data structures, error handling, naming) are not business rules and need no annotation.

For each business rule you write, ask: does something you were given or read decide this?

- **Yes, a source decides it.** Add `@source <ref>`. Add the quote that decides it if you can, and `by` if nearby code uses one.
- **No, you are choosing.** Add `@open "<question>"`. Do not pick silently, and do not write a `@source` for a source that doesn't actually decide it.
- **Not a business rule.** Add nothing.

In a governed area, every exported function, class or top-level constant needs at least one `@source` or `@open`. Outside governed areas, annotate only when a rule clearly came from somewhere specific.

## Reading and quoting sources

- Read every source with `npx live-comments read <ref>` or the `read_source` tool. Never quote from a web page, a pasted screenshot or another tool. The checker compares quotes against exactly what `read` returns.
- Copy quotes character for character from that output. Quote the shortest span that actually decides the rule.
- Quote the words of the person who decided, not a restatement. A comment from the product owner and a guess from an engineer in the same ticket are different sources of authority.

## Writing `@open`

Only the question is required. Make it answerable in one line:

```ts
/** @open "Do discount codes stack with gift cards?" options "yes" | "no, gift card applies first" */
```

- One decision per `@open`. Never write "TBD" or "check this".
- List the options you considered when there are a few.
- Add `by` and `in` the way nearby code does. If a ticket already discusses the question, point at it: `in linear:ENG-142`.

## Connecting sources

Sometimes a source lives in a tool the project can't read yet: `read` reports an unknown scheme or denied access, or the user says "the decision is in our Notion".

- Ask the user to connect it with `npx live-comments connect <system>`. Connecting needs the user to sign in, so you can't do it for them. Tell them exactly which command to run and why.
- Until it is connected, don't write a `@source` for something you couldn't read. Write an `@open` that points at where the answer lives, for example `in notion:space/Pricing`.

## Choosing `by`

`by` names who should decide, using names from the `AUTHORITY` file. Check what a name covers with `npx live-comments who <name>`.

**When the user says who decides** ("leave this to the pricing team", "legal owns refund rules"), find the matching name yourself. Run `npx live-comments authority` to see every name fully expanded, and match the user's words against names, members and places. "The pricing team" might be a group (`pricing`) or an authority that also fixes where they decide (`pricing-call`); prefer the one nearby code already uses.

- **One clear match:** use it, and mention it in your summary.
- **Several matches:** ask the user which one, showing what each covers.
- **No match:** propose a definition, for example `pricing-team = members(linear:team/PRICING)`. Get exact ids from `npx live-comments directory <system>` rather than guessing them. Show the definition to the user and add it to `AUTHORITY` only if they agree. Tell them the change needs review from the owners of `AUTHORITY`.

**When the user doesn't say,** reuse the authority that neighbouring code uses for the same kind of rule.

Rules that always apply:

- Never add or change a definition in `AUTHORITY` on your own initiative. Propose it and wait for the user.
- Never change an existing `by` unless the user asked you to. Authority changes are flagged in review.
- If you are unsure who decides, leave `by` off and say so in your summary.

## Answering an `@open`

When a source now decides an open question, replace the `@open` with a `@source` for it. If `context` reports `sticky: true`, keep the opening's `by` and `in`, combined as `by <who> in <where>`, and drop its `options` and id:

```ts
/** @open "Do discount codes stack?" by pricing in linear:team/PRICING options "yes" | "no" */
// becomes
/** @source linear:ENG-142 "Codes do not stack" by pricing in linear:team/PRICING */
```

`check` reports a replacement that dropped sticky arguments.

## Before you finish

Run `npx live-comments check`. Fix every error it reports in code you touched. In your summary to the user, list:

- every `@open` you added, with its question;
- any `by` you were unsure about;
- any rule you chose yourself because no source decided it.
