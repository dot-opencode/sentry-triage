---
name: sentry-triage
description: Record and re-check Sentry issue investigations with the `triage` CLI. Use when investigating a Sentry issue (short id like CORE-88), deciding whether it is a bug or expected, or when asked whether an issue was already looked at, reopened, or why it was closed.
license: MIT
---

# Sentry Triage

`triage` keeps one durable record per Sentry issue in `~/.sentry-triage/<org>/<ISSUE>/`
(override with `$SENTRY_TRIAGE_HOME`): `state.md` (current conclusion), `journal.md`
(append-only log), `evidence/` (raw dumps). It stores links to tracker tasks; it does
not create them. Run `triage help` for the command list.

## Protocol

1. **Look first.** `triage find <ISSUE>` and `triage show <ISSUE>`. Read prior conclusions
   before touching anything. A reopened issue means the previous fix did not hold: say so
   and find out why.
2. **Create the entry** if none exists: `triage new <ISSUE> --org <org> [--project p] [--url u] [--repo r] [--commit c]`.
3. **Gather exact Sentry data** (MCP or UI): stacktrace, event count, first/last seen,
   release, affected users. Save raw stacktraces/events to `$(triage path <ISSUE>)/evidence/`.
   Log every check with `triage log <ISSUE> "<what, where, result>"`.
4. **Inspect the code.** Record evidence in the `## Evidence` section of `state.md` as
   `path:line` plus the commit inspected (`triage set <ISSUE> repo=... commit=...`).
5. **Decide.** `triage set <ISSUE> verdict=... confidence=...` and fill the body sections
   (`## Error`, `## Root cause`, `## Evidence`, `## Decision`, `## Open questions`):
   - `bug` only with code evidence.
   - `expected` only when demonstrated to be expected and narrowly identifiable
     (never "all 4xx" or a whole error class).
   - otherwise `investigate`.
6. **Link tasks.** After another skill or the user creates a tracker task:
   `triage set <ISSUE> tracker+=<id-or-url>`. Issues sharing a root cause:
   `triage set <ISSUE> related+=<ISSUE>` (or `ORG/ISSUE`). Close with
   `triage set <ISSUE> status=closed` (needs a verdict other than `unknown`).
7. **Verify and save.** `triage check`, then `triage backup`.

## Rules

- Edit the body of `state.md` directly; change frontmatter only through `triage set`.
- Never rewrite `journal.md`; use `triage log`.
- Impact judgement (occurrences, users affected, fallback behaviour) belongs in `## Decision`.
- On Sentry rate limits (429): pace requests, fall back to source search, and log the
  limitation with `triage log`.
- With several orgs sharing an issue id, pass `--org`.
- Old per-issue directories can be imported once: `triage import <dir> --org <org>`.
