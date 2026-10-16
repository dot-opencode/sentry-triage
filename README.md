# sentry-triage

A skill and a small CLI (a single Rust binary) that record Sentry issue investigations — what was
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

Runtime dependencies: `git` (and `curl` or `wget` plus `tar` for the first-run download).

### How the binary is obtained

`skills/sentry-triage/bin/triage` is a small POSIX `sh` shim. It runs, in order:

1. `$SENTRY_TRIAGE_BIN`, if set (any binary; nothing is downloaded);
2. the cached `${XDG_CACHE_HOME:-~/.cache}/sentry-triage/<VERSION>/triage`;
3. otherwise it downloads `triage-<target>.tar.gz` and its `.sha256` from the
   GitHub release `v<VERSION>` (`<VERSION>` is `skills/sentry-triage/bin/VERSION`),
   verifies the checksum, caches the binary and runs it. Targets: macOS
   (arm64/x86_64) and Linux musl (x86_64/aarch64). `SENTRY_TRIAGE_RELEASE_BASE`
   overrides the download base URL.

If the download fails the shim exits 127 and prints the URL it tried, plus
`cargo install --git https://github.com/dot-opencode/sentry-triage --tag v<VERSION>`
and the `SENTRY_TRIAGE_BIN` alternative.

### Building from source

```bash
cargo build --release          # target/release/triage
SENTRY_TRIAGE_BIN=$PWD/target/release/triage bin/triage --version
```

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
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
cargo build --release
TRIAGE=target/release/triage bash tests/run.sh      # default TRIAGE is target/release/triage
```

Each black-box test runs in an isolated temp `HOME` and `SENTRY_TRIAGE_HOME`;
the suite also covers the shim. CI runs fmt, clippy, unit tests, the suite on
Linux and macOS, and shellcheck.

## Releasing

1. Bump `version` in `Cargo.toml` (and `Cargo.lock`) and put the same version in
   `skills/sentry-triage/bin/VERSION`.
2. Commit, then tag and push: `git tag vX.Y.Z && git push origin main vX.Y.Z`.
3. `release.yml` verifies tag = Cargo version = `VERSION`, builds the four
   targets and uploads `triage-<target>.tar.gz` + `.sha256` to the GitHub release.

## License

MIT
