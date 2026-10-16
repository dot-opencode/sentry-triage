# sentry-triage

A skill and a small Bash CLI that record Sentry issue investigations — what was
looked at, when, and what was decided — in one store outside your projects.
Past conclusions can then be found and re-checked later ("was API-42 already
investigated? why was it closed? did the fix hold?").

It is **personal** and **tracker-agnostic**: it stores links to YouTrack/Jira/etc.
tasks but never creates them. There is no JSON output and no plugin in v1.

The agent protocol lives in [`skills/sentry-triage/SKILL.md`](skills/sentry-triage/SKILL.md).

## Install

```bash
npx skills add dot-opencode/sentry-triage -g -a opencode
# optional: put the CLI on your PATH
ln -s ~/.agents/skills/sentry-triage/bin/triage ~/.local/bin/triage
triage init
```

Or from a checkout: `bin/triage` is a relative symlink to
`skills/sentry-triage/bin/triage`.

Dependencies: Bash 3.2+, `git`, `awk`, `sed`, `grep`, coreutils. `rg` is used
for `triage find` when present.

## Store

`$SENTRY_TRIAGE_HOME` (default `~/.sentry-triage`) — a git repo created by `triage init`.

```
<org>/<ISSUE>/
  state.md     # current conclusion: flat frontmatter + body
  journal.md   # append-only, timestamped log
  evidence/    # raw dumps: stacktraces, events, logs (free-form)
  legacy/      # files copied by `triage import`
```

`<org>` and `<ISSUE>` match `[A-Za-z0-9][A-Za-z0-9._-]*`. `ISSUE` is the Sentry
short id (`API-42`), stored upper-case.

### state.md frontmatter

Flat `key: value`, one line each.

| key | values |
| --- | --- |
| schema | `1` |
| issue | short id |
| org | Sentry org slug |
| project | Sentry project slug, may be empty |
| url | Sentry issue URL, may be empty |
| status | `open` \| `closed` |
| verdict | `unknown` \| `bug` \| `expected` \| `investigate` |
| confidence | empty \| `low` \| `medium` \| `high` |
| tracker | comma-separated ids/URLs, may be empty |
| related | comma-separated `ORG/ISSUE` or `ISSUE` (same org), may be empty |
| repo | path or remote of the code inspected, may be empty |
| commit | commit sha the code was inspected at, may be empty |
| created | `YYYY-MM-DD` |
| updated | `YYYY-MM-DD`, bumped by every CLI write |

The body has the sections `## Error`, `## Root cause`, `## Evidence`,
`## Decision`, `## Open questions`. Edit the body directly; change frontmatter
only through `triage set`.

### journal.md

Entries are `## YYYY-MM-DDTHH:MM:SSZ` (UTC) followed by text. Only the CLI
appends (`new`, `log`, `set`, `import`); nothing is rewritten.

## Commands

All commands exit `0` on success, `1` on user error or failed check, `2` on usage
error. Errors go to stderr. `ISSUE` lookup without `--org` works when exactly one
org has that issue; otherwise pass `--org`.

| command | what it does |
| --- | --- |
| `triage init` | Create the store and `git init` it. Idempotent. |
| `triage new <ISSUE> --org <org> [--project p] [--url u] [--repo r] [--commit c]` | Create an entry (auto-inits the store). Fails if it exists. |
| `triage log <ISSUE> [--org o] <text...>` | Append a journal entry. Use `-` as the text to read stdin. |
| `triage set <ISSUE> [--org o] key=value...` | Update `project url status verdict confidence tracker related repo commit` with validation. `tracker+=X` / `related+=X` append without duplicates. `status=closed` needs a verdict other than `unknown`. All pairs apply or none. |
| `triage show <ISSUE> [--org o]` | Print `state.md`, then `journal.md`. |
| `triage path <ISSUE> [--org o]` | Print the entry directory (for evidence files). |
| `triage list [--org o] [--status s] [--verdict v]` | `ORG/ISSUE  status  verdict  updated  url`, newest first. |
| `triage find <pattern>` | Case-insensitive fixed-string search over `state.md`, `journal.md` and `evidence/`; prints `ORG/ISSUE: file:line: text`. |
| `triage check` | Validate every entry (keys, values, path/frontmatter match, journal, `related` targets, closed ⇒ verdict). Exit 1 on problems. |
| `triage import <dir> --org <org> [--project p]` | Import legacy per-issue directories (see below). |
| `triage backup [--push]` | Commit store changes (no-op when clean); `--push` pushes `HEAD` to the first remote. |
| `triage --version`, `triage help` | |

## Migrating old notes: `triage import`

Each subdirectory of `<dir>` named like an issue id (`API-42`, `WEB-7`)
becomes an entry; others are reported as ignored and existing entries are skipped.
All files are copied into `legacy/`; the source directory is never modified.
Best-effort extraction from the legacy text:

- first `https://<org>.sentry.io/.../issues/...` URL → `url`
- YouTrack (`/issue/ID-1`) or Jira (`/browse/ID-1`) URLs → `tracker`
- `bug_candidate` → `bug`, `ignore_candidate` → `expected`,
  `needs_investigation` → `investigate` (otherwise `unknown`)
- a `Confidence:` line → `confidence`
- the journal records `imported from <abs path>`

```bash
triage import ./old-triage --org acme
triage check && triage list
```

Entries that were not extracted cleanly keep their original text in `legacy/`.

## Tests

```bash
bash tests/run.sh      # or: npm test
```

Each test runs in an isolated temp `HOME` and `SENTRY_TRIAGE_HOME`. CI runs shellcheck,
Linux, and macOS with bash 3.2 forced.

## License

MIT
