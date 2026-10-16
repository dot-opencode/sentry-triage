#!/usr/bin/env bash
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# Test the script the skill actually ships; the root bin/triage is only a convenience symlink.
TRIAGE="$ROOT/skills/sentry-triage/bin/triage"
FIX="$ROOT/tests/fixtures"
PASS=0; FAIL=0

setup() {
  set -e
  TMP="$(mktemp -d)"
  export SENTRY_TRIAGE_HOME="$TMP/store"
  export HOME="$TMP/home"                 # isolates global git config too
  unset XDG_CONFIG_HOME                   # CI runners set it; config must follow $HOME
  unset SENTRY_TRIAGE_NO_RG
  export GIT_CONFIG_NOSYSTEM=1
  mkdir -p "$HOME" "$TMP/work"
  git config --global user.email "test@example.com"
  git config --global user.name "Test"
  git config --global init.defaultBranch main
  cd "$TMP/work"
}

fail() { echo "  assertion failed: $*"; exit 1; }
assert_eq() { [ "$1" = "$2" ] || fail "expected '$2', got '$1'"; }
assert_contains() { case "$1" in *"$2"*) ;; *) fail "no '$2' in: $1" ;; esac; }
assert_not_contains() { case "$1" in *"$2"*) fail "unexpected '$2' in: $1" ;; *) ;; esac; }
assert_file() { [ -f "$1" ] || fail "missing file: $1"; }
assert_rc() {   # assert_rc <expected> <command...>; leaves output in OUT
  local want="$1" rc=0; shift
  OUT="$("$@" 2>&1)" || rc=$?
  [ "$rc" -eq "$want" ] || fail "expected exit $want, got $rc for '$*': $OUT"
}
fm() { awk -v k="$2" 'index($0, k ":") == 1 { v = substr($0, length(k) + 2); sub(/^ /, "", v); print v; exit }' "$1"; }

run_test() {
  (
    setup
    trap 'cd /; rm -rf "$TMP"' EXIT
    "$1"
  )
  local rc=$?
  if [ "$rc" -eq 0 ]; then PASS=$((PASS+1)); echo "ok - $1"
  else FAIL=$((FAIL+1)); echo "FAIL - $1"; fi
}

# ---------------- tests ----------------

test_version_help_and_usage_errors() {
  assert_rc 0 "$TRIAGE" --version
  assert_contains "$OUT" "triage 0."
  assert_rc 0 "$TRIAGE" help
  assert_contains "$OUT" "usage: triage"
  assert_rc 2 "$TRIAGE"
  assert_rc 2 "$TRIAGE" frobnicate
  assert_contains "$OUT" "unknown command"
  assert_rc 2 "$TRIAGE" new
  assert_rc 2 "$TRIAGE" new CORE-1
  assert_rc 2 "$TRIAGE" log
  assert_rc 2 "$TRIAGE" set
  assert_rc 2 "$TRIAGE" find
  assert_rc 2 "$TRIAGE" import
  assert_rc 2 "$TRIAGE" init extra
}

test_bin_symlink_is_relative_and_works() {
  [ -L "$ROOT/bin/triage" ] || fail "bin/triage is not a symlink"
  assert_eq "$(readlink "$ROOT/bin/triage")" "../skills/sentry-triage/bin/triage"
  assert_rc 0 "$ROOT/bin/triage" --version
}

test_init_creates_git_store_and_is_idempotent() {
  assert_rc 0 "$TRIAGE" init
  [ -d "$SENTRY_TRIAGE_HOME/.git" ] || fail "store is not a git repo"
  assert_rc 0 "$TRIAGE" init
}

test_default_store_is_under_home() {
  unset SENTRY_TRIAGE_HOME
  assert_rc 0 "$TRIAGE" init
  [ -d "$HOME/.sentry-triage/.git" ] || fail "default store missing"
}

test_relative_store_path_is_resolved_against_cwd() {
  export SENTRY_TRIAGE_HOME="rel-store"
  assert_rc 0 "$TRIAGE" new CORE-1 --org acme
  [ -f "$TMP/work/rel-store/acme/CORE-1/state.md" ] || fail "relative store not under cwd"
}

test_commands_fail_without_store() {
  assert_rc 1 "$TRIAGE" list
  assert_contains "$OUT" "triage init"
  assert_rc 1 "$TRIAGE" check
  assert_rc 1 "$TRIAGE" show CORE-1
  assert_rc 1 "$TRIAGE" find x
  assert_rc 1 "$TRIAGE" backup
}

test_new_creates_entry_with_template() {
  assert_rc 0 "$TRIAGE" new core-88 --org acme --project core --url https://acme.sentry.io/issues/123 \
    --repo git@example.com:acme/core.git --commit abc1234
  local d="$SENTRY_TRIAGE_HOME/acme/CORE-88" s
  s="$d/state.md"
  assert_file "$s"; assert_file "$d/journal.md"
  [ -d "$d/evidence" ] || fail "evidence/ missing"
  assert_eq "$(head -1 "$s")" "---"
  assert_eq "$(fm "$s" schema)" 1
  assert_eq "$(fm "$s" issue)" CORE-88
  assert_eq "$(fm "$s" org)" acme
  assert_eq "$(fm "$s" project)" core
  assert_eq "$(fm "$s" url)" "https://acme.sentry.io/issues/123"
  assert_eq "$(fm "$s" status)" open
  assert_eq "$(fm "$s" verdict)" unknown
  assert_eq "$(fm "$s" confidence)" ""
  assert_eq "$(fm "$s" repo)" "git@example.com:acme/core.git"
  assert_eq "$(fm "$s" commit)" abc1234
  assert_eq "$(fm "$s" created)" "$(date -u +%Y-%m-%d)"
  assert_eq "$(fm "$s" updated)" "$(fm "$s" created)"
  local h
  for h in "## Error" "## Root cause" "## Evidence" "## Decision" "## Open questions"; do
    grep -qxF "$h" "$s" || fail "missing section $h"
  done
  grep -qE '^## [0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$' "$d/journal.md" || fail "journal timestamp format"
  assert_contains "$(<"$d/journal.md")" "created"
  assert_rc 0 "$TRIAGE" check
}

test_new_validation_and_duplicates() {
  assert_rc 0 "$TRIAGE" new CORE-1 --org acme
  assert_rc 1 "$TRIAGE" new CORE-1 --org acme
  assert_contains "$OUT" "already exists"
  assert_rc 1 "$TRIAGE" new core-1 --org acme            # case-insensitive duplicate
  assert_rc 1 "$TRIAGE" new "../evil" --org acme
  assert_rc 1 "$TRIAGE" new "bad id" --org acme
  assert_rc 1 "$TRIAGE" new CORE-2 --org "../x"
  assert_rc 1 "$TRIAGE" new CORE-2 --org acme --url notaurl
  assert_rc 1 "$TRIAGE" new CORE-2 --org acme --commit zzz
  assert_rc 2 "$TRIAGE" new CORE-2 --bogus x --org acme
  assert_rc 2 "$TRIAGE" new CORE-2 --org
  [ ! -e "$SENTRY_TRIAGE_HOME/acme/CORE-2" ] || fail "failed new left an entry"
}

test_set_updates_validates_and_journals() {
  "$TRIAGE" new CORE-1 --org acme >/dev/null
  local s="$SENTRY_TRIAGE_HOME/acme/CORE-1/state.md"
  assert_rc 0 "$TRIAGE" set CORE-1 verdict=bug confidence=high project=core url=https://x.example/i/1 repo=/src/core commit=deadbeef
  assert_eq "$(fm "$s" verdict)" bug
  assert_eq "$(fm "$s" confidence)" high
  assert_eq "$(fm "$s" project)" core
  assert_eq "$(fm "$s" repo)" /src/core
  assert_eq "$(fm "$s" commit)" deadbeef
  assert_contains "$("$TRIAGE" show CORE-1)" "set verdict=bug"
  assert_rc 0 "$TRIAGE" set CORE-1 confidence=
  assert_eq "$(fm "$s" confidence)" ""
  # invalid values: nothing is applied
  assert_rc 1 "$TRIAGE" set CORE-1 verdict=maybe
  assert_rc 1 "$TRIAGE" set CORE-1 status=done
  assert_rc 1 "$TRIAGE" set CORE-1 confidence=extreme
  assert_rc 1 "$TRIAGE" set CORE-1 url=nope
  assert_rc 1 "$TRIAGE" set CORE-1 commit=xyz
  assert_rc 1 "$TRIAGE" set CORE-1 schema=2
  assert_contains "$OUT" "cannot be set"
  assert_rc 1 "$TRIAGE" set CORE-1 issue=OTHER-1
  assert_rc 1 "$TRIAGE" set CORE-1 updated=2020-01-01
  assert_rc 1 "$TRIAGE" set CORE-1 unknownkey=x
  assert_rc 1 "$TRIAGE" set CORE-1 verdict+=bug
  assert_rc 1 "$TRIAGE" set CORE-1 "tracker+=has space"
  assert_rc 2 "$TRIAGE" set CORE-1 notkeyvalue
  assert_rc 2 "$TRIAGE" set CORE-1
  assert_rc 1 "$TRIAGE" set NOPE-1 verdict=bug
  assert_rc 0 "$TRIAGE" check
}

test_set_is_atomic_when_one_pair_is_invalid() {
  "$TRIAGE" new CORE-1 --org acme >/dev/null
  local s="$SENTRY_TRIAGE_HOME/acme/CORE-1/state.md"
  assert_rc 1 "$TRIAGE" set CORE-1 verdict=bug confidence=bogus
  assert_eq "$(fm "$s" verdict)" unknown
}

test_set_lists_append_without_duplicates() {
  "$TRIAGE" new CORE-1 --org acme >/dev/null
  "$TRIAGE" new CORE-2 --org acme >/dev/null
  local s="$SENTRY_TRIAGE_HOME/acme/CORE-1/state.md"
  assert_rc 0 "$TRIAGE" set CORE-1 tracker+=PROJ-1 tracker+=PROJ-2 tracker+=PROJ-1
  assert_eq "$(fm "$s" tracker)" "PROJ-1, PROJ-2"
  assert_rc 0 "$TRIAGE" set CORE-1 tracker+=https://tracker.example.com/issue/PROJ-3
  assert_eq "$(fm "$s" tracker)" "PROJ-1, PROJ-2, https://tracker.example.com/issue/PROJ-3"
  assert_rc 0 "$TRIAGE" set CORE-1 related+=core-2 related+=CORE-2
  assert_eq "$(fm "$s" related)" "CORE-2"
  assert_rc 0 "$TRIAGE" set CORE-1 related+=acme/CORE-2
  assert_eq "$(fm "$s" related)" "CORE-2, acme/CORE-2"
  assert_rc 0 "$TRIAGE" set CORE-1 tracker=ONLY-1
  assert_eq "$(fm "$s" tracker)" "ONLY-1"
  assert_rc 0 "$TRIAGE" check
}

test_set_bumps_updated_and_close_requires_verdict() {
  "$TRIAGE" new CORE-1 --org acme >/dev/null
  local s="$SENTRY_TRIAGE_HOME/acme/CORE-1/state.md" tmp
  tmp="$s.tmp"
  sed 's/^updated: .*/updated: 2020-01-01/; s/^created: .*/created: 2020-01-01/' "$s" > "$tmp" && mv "$tmp" "$s"
  assert_rc 1 "$TRIAGE" set CORE-1 status=closed
  assert_contains "$OUT" "verdict"
  assert_eq "$(fm "$s" status)" open
  assert_eq "$(fm "$s" updated)" 2020-01-01          # failed write must not bump
  assert_rc 0 "$TRIAGE" set CORE-1 status=closed verdict=expected
  assert_eq "$(fm "$s" status)" closed
  assert_eq "$(fm "$s" updated)" "$(date -u +%Y-%m-%d)"
  assert_rc 1 "$TRIAGE" set CORE-1 verdict=unknown   # would leave a closed entry without verdict
  assert_rc 0 "$TRIAGE" set CORE-1 status=open verdict=unknown
}

test_log_args_stdin_and_errors() {
  "$TRIAGE" new CORE-1 --org acme >/dev/null
  local j="$SENTRY_TRIAGE_HOME/acme/CORE-1/journal.md"
  assert_rc 0 "$TRIAGE" log CORE-1 checked the stacktrace in Sentry
  assert_contains "$(<"$j")" "checked the stacktrace in Sentry"
  assert_rc 0 "$TRIAGE" log --org acme CORE-1 "with org flag"
  assert_contains "$(<"$j")" "with org flag"
  OUT="$(printf 'line one\nline two\n' | "$TRIAGE" log CORE-1 -)"
  assert_contains "$(<"$j")" "line two"
  assert_rc 2 "$TRIAGE" log CORE-1
  assert_rc 1 "$TRIAGE" log NOPE-1 text
  # append-only: the first entry is still there
  assert_contains "$(<"$j")" "created"
  assert_eq "$(grep -c '^## ' "$j")" 4
}

test_show_path_and_lookup_rules() {
  "$TRIAGE" new CORE-1 --org acme --url https://acme.sentry.io/issues/1 >/dev/null
  assert_rc 0 "$TRIAGE" show core-1
  assert_contains "$OUT" "issue: CORE-1"
  assert_contains "$OUT" "# Journal acme/CORE-1"
  assert_contains "$OUT" "created"
  assert_rc 0 "$TRIAGE" path CORE-1
  assert_eq "$OUT" "$SENTRY_TRIAGE_HOME/acme/CORE-1"
  assert_rc 0 "$TRIAGE" path --org acme CORE-1
  assert_rc 1 "$TRIAGE" path CORE-1 --org other
  assert_rc 1 "$TRIAGE" show NOPE-1
  assert_contains "$OUT" "no entry"
  assert_rc 1 "$TRIAGE" show "../x"
}

test_ambiguous_org_lookup() {
  "$TRIAGE" new CORE-1 --org acme >/dev/null
  "$TRIAGE" new CORE-1 --org other >/dev/null
  assert_rc 1 "$TRIAGE" show CORE-1
  assert_contains "$OUT" "--org"
  assert_contains "$OUT" "acme"
  assert_contains "$OUT" "other"
  assert_rc 1 "$TRIAGE" set CORE-1 verdict=bug
  assert_rc 1 "$TRIAGE" log CORE-1 hi
  assert_rc 1 "$TRIAGE" path CORE-1
  assert_rc 0 "$TRIAGE" show CORE-1 --org other
  assert_contains "$OUT" "org: other"
  assert_rc 0 "$TRIAGE" log --org acme CORE-1 hi
}

test_list_filters_and_sorting() {
  "$TRIAGE" new CORE-1 --org acme --url https://acme.sentry.io/issues/1 >/dev/null
  "$TRIAGE" new CORE-2 --org acme >/dev/null
  "$TRIAGE" new WEB-1 --org other >/dev/null
  "$TRIAGE" set CORE-2 verdict=bug >/dev/null
  "$TRIAGE" set WEB-1 verdict=expected status=closed >/dev/null
  # force distinct updated dates
  local f
  f="$SENTRY_TRIAGE_HOME/acme/CORE-1/state.md"
  sed 's/^updated: .*/updated: 2026-01-01/' "$f" > "$f.t" && mv "$f.t" "$f"
  f="$SENTRY_TRIAGE_HOME/acme/CORE-2/state.md"
  sed 's/^updated: .*/updated: 2026-03-01/' "$f" > "$f.t" && mv "$f.t" "$f"
  f="$SENTRY_TRIAGE_HOME/other/WEB-1/state.md"
  sed 's/^updated: .*/updated: 2026-02-01/' "$f" > "$f.t" && mv "$f.t" "$f"
  assert_rc 0 "$TRIAGE" list
  assert_eq "$(printf '%s\n' "$OUT" | cut -d' ' -f1 | paste -sd, -)" "acme/CORE-2,other/WEB-1,acme/CORE-1"
  assert_contains "$OUT" "acme/CORE-1  open  unknown  2026-01-01  https://acme.sentry.io/issues/1"
  assert_rc 0 "$TRIAGE" list --org other
  assert_eq "$OUT" "other/WEB-1  closed  expected  2026-02-01"
  assert_rc 0 "$TRIAGE" list --status closed
  assert_eq "$(printf '%s\n' "$OUT" | wc -l | tr -d ' ')" 1
  assert_rc 0 "$TRIAGE" list --verdict bug
  assert_contains "$OUT" "acme/CORE-2"
  assert_not_contains "$OUT" "CORE-1"
  assert_rc 0 "$TRIAGE" list --org acme --verdict investigate
  assert_eq "$OUT" ""
  assert_rc 1 "$TRIAGE" list --status bogus
  assert_rc 1 "$TRIAGE" list --verdict bogus
  assert_rc 2 "$TRIAGE" list --org
  assert_rc 2 "$TRIAGE" list stray
}

find_fixture() {
  "$TRIAGE" new CORE-1 --org acme >/dev/null
  "$TRIAGE" new WEB-1 --org other >/dev/null
  "$TRIAGE" log CORE-1 "Redis ConnectionError seen in worker" >/dev/null
  printf 'Traceback line\nTimeoutError: Pool Exhausted\n' > "$("$TRIAGE" path CORE-1)/evidence/trace.txt"
}

test_find_searches_state_journal_and_evidence() {
  find_fixture
  assert_rc 0 "$TRIAGE" find "redis connectionerror"
  assert_contains "$OUT" "acme/CORE-1: journal.md:"
  assert_contains "$OUT" "Redis ConnectionError seen in worker"
  assert_rc 0 "$TRIAGE" find "pool exhausted"
  assert_contains "$OUT" "acme/CORE-1: evidence/trace.txt:2:TimeoutError: Pool Exhausted"
  assert_rc 0 "$TRIAGE" find "issue: WEB-1"
  assert_contains "$OUT" "other/WEB-1: state.md:"
  assert_rc 0 "$TRIAGE" find "no-such-text-anywhere"
  assert_eq "$OUT" ""
}

test_find_is_fixed_string_and_grep_fallback_matches() {
  find_fixture
  assert_rc 0 "$TRIAGE" find "Traceback line"
  assert_contains "$OUT" "trace.txt:1:Traceback line"
  assert_rc 0 "$TRIAGE" find "Error.*"          # regex metachars are literal
  assert_eq "$OUT" ""
  assert_rc 0 "$TRIAGE" find "TimeoutError: Pool"
  local with_rg="$OUT"
  export SENTRY_TRIAGE_NO_RG=1
  assert_rc 0 "$TRIAGE" find "TimeoutError: Pool"
  assert_eq "$OUT" "$with_rg"
}

test_check_ok_and_counts() {
  assert_rc 0 "$TRIAGE" init
  assert_rc 0 "$TRIAGE" check
  assert_contains "$OUT" "OK (0 entries)"
  "$TRIAGE" new CORE-1 --org acme >/dev/null
  "$TRIAGE" new CORE-2 --org acme >/dev/null
  "$TRIAGE" set CORE-1 related+=CORE-2 verdict=bug status=closed >/dev/null
  assert_rc 0 "$TRIAGE" check
  assert_contains "$OUT" "OK (2 entries)"
}

edit_state() {   # edit_state <ORG/ISSUE> <sed-expr>
  local f="$SENTRY_TRIAGE_HOME/$1/state.md"
  sed "$2" "$f" > "$f.t" && mv "$f.t" "$f"
}

test_check_detects_each_problem() {
  "$TRIAGE" new CORE-1 --org acme >/dev/null
  local s="$SENTRY_TRIAGE_HOME/acme/CORE-1/state.md" good
  good="$(<"$s")"
  reset() { printf '%s\n' "$good" > "$s"; }

  edit_state acme/CORE-1 's/^verdict: .*/verdict: nonsense/'
  assert_rc 1 "$TRIAGE" check; assert_contains "$OUT" "acme/CORE-1: invalid verdict"; reset

  edit_state acme/CORE-1 's/^status: .*/status: weird/'
  assert_rc 1 "$TRIAGE" check; assert_contains "$OUT" "invalid status"; reset

  edit_state acme/CORE-1 's/^confidence:.*/confidence: huge/'
  assert_rc 1 "$TRIAGE" check; assert_contains "$OUT" "invalid confidence"; reset

  edit_state acme/CORE-1 's/^issue: .*/issue: CORE-9/'
  assert_rc 1 "$TRIAGE" check; assert_contains "$OUT" "does not match directory"; reset

  edit_state acme/CORE-1 's/^org: .*/org: elsewhere/'
  assert_rc 1 "$TRIAGE" check; assert_contains "$OUT" "org 'elsewhere' does not match"; reset

  edit_state acme/CORE-1 's/^created: .*/created: yesterday/'
  assert_rc 1 "$TRIAGE" check; assert_contains "$OUT" "invalid created date"; reset

  edit_state acme/CORE-1 's/^updated: .*/updated: 2000-01-01/'
  assert_rc 1 "$TRIAGE" check; assert_contains "$OUT" "before created"; reset

  edit_state acme/CORE-1 's/^schema: .*/schema: 7/'
  assert_rc 1 "$TRIAGE" check; assert_contains "$OUT" "schema must be 1"; reset

  edit_state acme/CORE-1 's/^url:.*/url: ftp:\/\/x/'
  assert_rc 1 "$TRIAGE" check; assert_contains "$OUT" "invalid url"; reset

  edit_state acme/CORE-1 's/^related:.*/related: GHOST-1/'
  assert_rc 1 "$TRIAGE" check; assert_contains "$OUT" "related target 'GHOST-1' does not exist"; reset

  edit_state acme/CORE-1 's/^related:.*/related: nowhere\/GHOST-1/'
  assert_rc 1 "$TRIAGE" check; assert_contains "$OUT" "related target 'nowhere/GHOST-1'"; reset

  edit_state acme/CORE-1 's/^status: .*/status: closed/'
  assert_rc 1 "$TRIAGE" check; assert_contains "$OUT" "closed entry has verdict unknown"; reset

  edit_state acme/CORE-1 '/^project:/d'
  assert_rc 1 "$TRIAGE" check; assert_contains "$OUT" "missing key project"; reset

  edit_state acme/CORE-1 's/^verdict: .*/bogusline/'
  assert_rc 1 "$TRIAGE" check; assert_contains "$OUT" "malformed"; reset

  edit_state acme/CORE-1 's/^repo:.*/extra: x\
&/'
  assert_rc 1 "$TRIAGE" check; assert_contains "$OUT" "unknown key extra"; reset

  awk '{ print } /^commit:/ { print "commit: abc1234" }' "$s" > "$s.t" && mv "$s.t" "$s"
  assert_rc 1 "$TRIAGE" check; assert_contains "$OUT" "duplicate key commit"; reset

  tail -n +2 "$s" > "$s.t" && mv "$s.t" "$s"
  assert_rc 1 "$TRIAGE" check; assert_contains "$OUT" "missing opening ---"; reset

  sed '16d' "$s" > "$s.t" && mv "$s.t" "$s"
  assert_rc 1 "$TRIAGE" check; assert_contains "$OUT" "missing closing ---"; reset

  rm "$SENTRY_TRIAGE_HOME/acme/CORE-1/journal.md"
  assert_rc 1 "$TRIAGE" check; assert_contains "$OUT" "journal.md is missing"
  printf '# Journal\n' > "$SENTRY_TRIAGE_HOME/acme/CORE-1/journal.md"

  rm "$s"
  assert_rc 1 "$TRIAGE" check; assert_contains "$OUT" "state.md is missing"
  printf '%s\n' "$good" > "$s"

  assert_rc 0 "$TRIAGE" check
}

test_check_reports_every_entry_and_count() {
  "$TRIAGE" new CORE-1 --org acme >/dev/null
  "$TRIAGE" new CORE-2 --org acme >/dev/null
  edit_state acme/CORE-1 's/^verdict: .*/verdict: x/'
  edit_state acme/CORE-2 's/^status: .*/status: y/'
  assert_rc 1 "$TRIAGE" check
  assert_contains "$OUT" "acme/CORE-1:"
  assert_contains "$OUT" "acme/CORE-2:"
  assert_contains "$OUT" "2 problem(s) in 2 entries"
}

test_import_fixture_with_bug_candidate() {
  assert_rc 0 "$TRIAGE" import "$FIX/legacy-a" --org acme --project core
  assert_contains "$OUT" "imported acme/CORE-1"
  assert_contains "$OUT" "imported acme/PROD-AB"
  assert_contains "$OUT" "ignored notes"
  assert_contains "$OUT" "2 imported, 0 skipped, 1 ignored"
  local d="$SENTRY_TRIAGE_HOME/acme/CORE-1" s
  s="$d/state.md"
  assert_eq "$(fm "$s" verdict)" bug
  assert_eq "$(fm "$s" url)" "https://acme.sentry.io/issues/CORE-1"
  assert_eq "$(fm "$s" tracker)" "https://tracker.example.com/issue/ACME-123"
  assert_eq "$(fm "$s" confidence)" high
  assert_eq "$(fm "$s" project)" core
  assert_eq "$(fm "$s" status)" open
  assert_file "$d/legacy/state.md"
  assert_file "$d/legacy/youtrack.md"
  assert_contains "$(<"$d/journal.md")" "imported from $FIX/legacy-a/CORE-1"
  case "$(<"$d/journal.md")" in *"\$'"*) fail "literal \$'\\n' in journal" ;; esac
  [ -d "$d/evidence" ] || fail "evidence/ missing"
  # ignore_candidate, trailing dot stripped from url
  s="$SENTRY_TRIAGE_HOME/acme/PROD-AB/state.md"
  assert_eq "$(fm "$s" verdict)" expected
  assert_eq "$(fm "$s" url)" "https://acme.sentry.io/issues/PROD-AB"
  assert_eq "$(fm "$s" confidence)" medium
  [ ! -e "$SENTRY_TRIAGE_HOME/acme/notes" ] || fail "non-issue dir imported"
  assert_rc 0 "$TRIAGE" check
}

test_import_fixture_with_needs_investigation() {
  assert_rc 0 "$TRIAGE" import "$FIX/legacy-b" --org acme
  local d="$SENTRY_TRIAGE_HOME/acme/WEB-7" s
  s="$d/state.md"
  assert_eq "$(fm "$s" verdict)" investigate
  assert_eq "$(fm "$s" confidence)" low
  assert_eq "$(fm "$s" url)" "https://acme.sentry.io/issues/WEB-7"
  assert_eq "$(fm "$s" tracker)" "https://jira.example.com/browse/ACME-9"
  assert_file "$d/legacy/sentry-raw.md"
  # Sentry project slugs may contain underscores.
  assert_eq "$(fm "$SENTRY_TRIAGE_HOME/acme/AUDIO_STORAGE-4P/state.md" verdict)" expected
  assert_rc 0 "$TRIAGE" check
}

test_import_skips_existing_never_modifies_source_and_validates() {
  local copy="$TMP/legacy-copy"
  cp -R "$FIX/legacy-a" "$copy"
  local before
  before="$(cd "$copy" && find . -type f -exec cksum {} + | sort)"
  assert_rc 0 "$TRIAGE" import "$copy" --org acme
  assert_rc 0 "$TRIAGE" import "$copy" --org acme
  assert_contains "$OUT" "skipped acme/CORE-1 (exists)"
  assert_contains "$OUT" "0 imported, 2 skipped"
  assert_eq "$(cd "$copy" && find . -type f -exec cksum {} + | sort)" "$before"
  assert_eq "$(find "$copy" -type f | wc -l | tr -d ' ')" "$(find "$FIX/legacy-a" -type f | wc -l | tr -d ' ')"
  assert_eq "$(grep -c '^## ' "$SENTRY_TRIAGE_HOME/acme/CORE-1/journal.md")" 1
  assert_rc 1 "$TRIAGE" import "$TMP/does-not-exist" --org acme
  assert_rc 2 "$TRIAGE" import "$copy"
  assert_rc 1 "$TRIAGE" import "$copy" --org "bad/org"
  assert_rc 1 "$TRIAGE" import "$copy" --org acme --project "bad project"
}

test_backup_commits_and_is_noop_when_clean() {
  "$TRIAGE" new CORE-1 --org acme >/dev/null
  assert_rc 0 "$TRIAGE" backup
  assert_contains "$OUT" "committed"
  assert_eq "$(git -C "$SENTRY_TRIAGE_HOME" rev-list --count HEAD)" 1
  assert_eq "$(git -C "$SENTRY_TRIAGE_HOME" status --porcelain)" ""
  git -C "$SENTRY_TRIAGE_HOME" ls-files | grep -qx 'acme/CORE-1/state.md' || fail "state.md not tracked"
  assert_rc 0 "$TRIAGE" backup
  assert_contains "$OUT" "nothing to commit"
  assert_eq "$(git -C "$SENTRY_TRIAGE_HOME" rev-list --count HEAD)" 1
  "$TRIAGE" log CORE-1 more >/dev/null
  assert_rc 0 "$TRIAGE" backup
  assert_eq "$(git -C "$SENTRY_TRIAGE_HOME" rev-list --count HEAD)" 2
  assert_rc 2 "$TRIAGE" backup --bogus
}

test_backup_push_needs_remote_and_pushes() {
  "$TRIAGE" new CORE-1 --org acme >/dev/null
  assert_rc 1 "$TRIAGE" backup --push
  assert_contains "$OUT" "no git remote"
  git init -q --bare "$TMP/remote.git"
  git -C "$SENTRY_TRIAGE_HOME" remote add origin "$TMP/remote.git"
  assert_rc 0 "$TRIAGE" backup --push
  assert_contains "$OUT" "pushed"
  assert_eq "$(git -C "$TMP/remote.git" rev-list --count HEAD)" 1
}

test_backup_explains_commit_failure() {
  "$TRIAGE" new CORE-1 --org acme >/dev/null
  git config --global --unset user.email
  git config --global user.useConfigOnly true
  assert_rc 1 "$TRIAGE" backup
  assert_contains "$OUT" "git commit failed"
}

# ---------------- runner ----------------

for t in $(declare -F | awk '{print $3}' | grep '^test_'); do run_test "$t"; done
echo "passed $PASS, failed $FAIL"
[ "$FAIL" -eq 0 ]
