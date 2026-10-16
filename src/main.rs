//! sentry-triage: record Sentry issue investigations in a git-backed store.
//! Behaviour mirrors the original bash CLI (v0.1.0).

mod extract;
mod util;

use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use util::*;

const VERSION: &str = env!("CARGO_PKG_VERSION");

const USAGE: &str = "usage: triage <command> [args]        (triage --version prints the version)

commands:
  init                                      create the store (a git repo)
  new <ISSUE> --org <org> [--project p] [--url u] [--repo r] [--commit c]
                                            create an entry
  log <ISSUE> [--org o] <text...|->         append a journal entry (- reads stdin)
  set <ISSUE> [--org o] key=value...        update frontmatter (tracker+=X, related+=X append)
  show <ISSUE> [--org o]                    print state.md and journal.md
  path <ISSUE> [--org o]                    print the entry directory
  list [--org o] [--status s] [--verdict v] one line per entry, newest first
  find <pattern>                            case-insensitive fixed-string search
  check                                     validate every entry
  import <dir> --org <org> [--project p]    import legacy per-issue directories
  backup [--push]                           commit (and push) the store
  help                                      this text

store: $SENTRY_TRIAGE_HOME (default ~/.sentry-triage)
exit codes: 0 ok, 1 error / check failed, 2 usage error
";

const SETTABLE: &str = "project url status verdict confidence tracker related repo commit";

type Out<'a> = &'a mut dyn Write;

struct Ctx {
    store: String,
}

struct Entry {
    org: String,
    issue: String,
    dir: String,
}

fn io_err<T>(r: io::Result<T>, what: &str) -> R<T> {
    match r {
        Ok(v) => Ok(v),
        Err(e) => die(format!("{what}: {e}")),
    }
}

fn read_lossy(path: &str) -> io::Result<String> {
    Ok(String::from_utf8_lossy(&fs::read(path)?).into_owned())
}

fn is_file(p: &str) -> bool {
    Path::new(p).is_file()
}

fn is_dir(p: &str) -> bool {
    Path::new(p).is_dir()
}

/// Names of non-hidden subdirectories (symlinks followed), sorted bytewise.
fn subdirs(path: &str) -> Vec<String> {
    let mut v: Vec<String> = match fs::read_dir(path) {
        Ok(rd) => rd
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                (!name.starts_with('.') && e.path().is_dir()).then_some(name)
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    v.sort();
    v
}

impl Ctx {
    fn from_env() -> R<Ctx> {
        let store = match std::env::var("SENTRY_TRIAGE_HOME") {
            Ok(s) if !s.is_empty() => s,
            _ => match std::env::var("HOME") {
                Ok(h) if !h.is_empty() => format!("{h}/.sentry-triage"),
                _ => return die("HOME is not set"),
            },
        };
        let store = if store.starts_with('/') {
            store
        } else {
            let cwd = io_err(std::env::current_dir(), "cannot get current directory")?;
            format!("{}/{}", cwd.display(), store)
        };
        Ok(Ctx { store })
    }

    fn require_store(&self) -> R<()> {
        if is_dir(&self.store) {
            Ok(())
        } else {
            die(format!("no store at {} (run 'triage init')", self.store))
        }
    }

    fn ensure_store(&self) -> R<()> {
        io_err(fs::create_dir_all(&self.store), "cannot create store")?;
        if !is_dir(&format!("{}/.git", self.store)) {
            let ok = Command::new("git")
                .args(["-C", &self.store, "init", "-q"])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if !ok {
                return die("git init failed");
            }
        }
        Ok(())
    }

    /// (org, issue, dir) for every `STORE/*/*/` directory.
    fn all_dirs(&self) -> Vec<(String, String, String)> {
        let mut v = Vec::new();
        for o in subdirs(&self.store) {
            for i in subdirs(&format!("{}/{}", self.store, o)) {
                let d = format!("{}/{}/{}", self.store, o, i);
                v.push((o.clone(), i, d));
            }
        }
        v
    }

    fn resolve_entry(&self, issue: &str, org: &str) -> R<Entry> {
        if !valid_id(issue) {
            return die(format!("invalid issue id '{issue}'"));
        }
        let issue = upper(issue);
        if !org.is_empty() {
            if !valid_id(org) {
                return die(format!("invalid org '{org}'"));
            }
            if !is_file(&format!("{}/{org}/{issue}/state.md", self.store)) {
                return die(format!("no entry {org}/{issue}"));
            }
            let dir = format!("{}/{org}/{issue}", self.store);
            return Ok(Entry {
                org: org.to_string(),
                issue,
                dir,
            });
        }
        let mut found: Vec<String> = Vec::new();
        for o in subdirs(&self.store) {
            if is_file(&format!("{}/{o}/{issue}/state.md", self.store)) {
                found.push(o);
            }
        }
        match found.len() {
            0 => die(format!("no entry for {issue}")),
            1 => {
                let org = found.remove(0);
                let dir = format!("{}/{org}/{issue}", self.store);
                Ok(Entry { org, issue, dir })
            }
            _ => die(format!(
                "{issue} exists in several orgs ({}); pass --org",
                found.join(", ")
            )),
        }
    }
}

// --------------------------------------------------------------- entries

#[allow(clippy::too_many_arguments)]
fn create_entry(
    ctx: &Ctx,
    org: &str,
    issue: &str,
    project: &str,
    url: &str,
    repo: &str,
    commit: &str,
    tracker: &str,
    verdict: &str,
    confidence: &str,
) -> R<()> {
    let dir = format!("{}/{org}/{issue}", ctx.store);
    if fs::symlink_metadata(&dir).is_ok() {
        return die(format!("entry {org}/{issue} already exists"));
    }
    io_err(
        fs::create_dir_all(format!("{dir}/evidence")),
        "cannot create entry",
    )?;
    let t = today();
    let mut s = String::from("---\n");
    let mut kv = |k: &str, v: &str| {
        if v.is_empty() {
            s.push_str(&format!("{k}:\n"));
        } else {
            s.push_str(&format!("{k}: {v}\n"));
        }
    };
    kv("schema", "1");
    kv("issue", issue);
    kv("org", org);
    kv("project", project);
    kv("url", url);
    kv("status", "open");
    kv("verdict", verdict);
    kv("confidence", confidence);
    kv("tracker", tracker);
    kv("related", "");
    kv("repo", repo);
    kv("commit", commit);
    kv("created", &t);
    kv("updated", &t);
    s.push_str(
        "---\n\n## Error\n\n## Root cause\n\n## Evidence\n\n## Decision\n\n## Open questions\n",
    );
    io_err(
        fs::write(format!("{dir}/state.md"), s),
        "cannot write state.md",
    )?;
    io_err(
        fs::write(
            format!("{dir}/journal.md"),
            format!("# Journal {org}/{issue}\n"),
        ),
        "cannot write journal.md",
    )
}

fn journal_append(dir: &str, text: &str) -> R<()> {
    let path = format!("{dir}/journal.md");
    let mut f = io_err(
        fs::OpenOptions::new().create(true).append(true).open(&path),
        "cannot open journal",
    )?;
    io_err(
        f.write_all(format!("\n## {}\n{}\n", now_utc(), text).as_bytes()),
        "cannot write journal",
    )
}

// ------------------------------------------------------------ arg parsing

struct IssueArgs {
    issue: String,
    org: String,
    rest: Vec<String>,
}

/// Parse `<ISSUE> [--org o]` in any order; stops at the first extra word.
fn parse_issue_args(cmd: &str, args: &[String]) -> R<IssueArgs> {
    let mut issue = String::new();
    let mut org = String::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--org" {
            if i + 1 >= args.len() || args[i + 1].is_empty() {
                return usage_err(format!("{cmd}: --org requires a value"));
            }
            org = args[i + 1].clone();
            i += 2;
        } else if issue.is_empty() {
            issue = args[i].clone();
            i += 1;
        } else {
            break;
        }
    }
    if issue.is_empty() {
        return usage_err(format!("{cmd}: missing <ISSUE>"));
    }
    Ok(IssueArgs {
        issue,
        org,
        rest: args[i..].to_vec(),
    })
}

// -------------------------------------------------------------- commands

fn cmd_init(ctx: &Ctx, args: &[String], out: Out) -> R<()> {
    if !args.is_empty() {
        return usage_err("usage: triage init");
    }
    ctx.ensure_store()?;
    let _ = writeln!(out, "initialized: {}", ctx.store);
    Ok(())
}

fn cmd_new(ctx: &Ctx, args: &[String], out: Out) -> R<()> {
    let (mut issue, mut org, mut project, mut url, mut repo, mut commit) = (
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
    );
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--org" | "--project" | "--url" | "--repo" | "--commit" => {
                if i + 1 >= args.len() {
                    return usage_err(format!("new: {a} requires a value"));
                }
                let v = args[i + 1].clone();
                match a {
                    "--org" => org = v,
                    "--project" => project = v,
                    "--url" => url = v,
                    "--repo" => repo = v,
                    _ => commit = v,
                }
                i += 2;
            }
            _ if a.starts_with('-') => return usage_err(format!("new: unknown option: {a}")),
            _ => {
                if !issue.is_empty() {
                    return usage_err(format!("new: unexpected argument: {a}"));
                }
                issue = a.to_string();
                i += 1;
            }
        }
    }
    if issue.is_empty() {
        return usage_err(
            "usage: triage new <ISSUE> --org <org> [--project p] [--url u] [--repo r] [--commit c]",
        );
    }
    if org.is_empty() {
        return usage_err("new: --org is required");
    }
    if !valid_id(&issue) {
        return die(format!("invalid issue id '{issue}'"));
    }
    if !valid_id(&org) {
        return die(format!("invalid org '{org}'"));
    }
    let issue = upper(&issue);
    for (k, v) in [
        ("project", &project),
        ("url", &url),
        ("repo", &repo),
        ("commit", &commit),
    ] {
        let err = field_error(k, v);
        if !err.is_empty() {
            return die(err.join("\n"));
        }
    }
    ctx.ensure_store()?;
    create_entry(
        ctx, &org, &issue, &project, &url, &repo, &commit, "", "unknown", "",
    )?;
    journal_append(&format!("{}/{org}/{issue}", ctx.store), "created")?;
    let _ = writeln!(out, "created {org}/{issue}");
    Ok(())
}

fn cmd_log(ctx: &Ctx, args: &[String], out: Out) -> R<()> {
    let pa = parse_issue_args("log", args)?;
    if pa.rest.is_empty() {
        return usage_err("usage: triage log <ISSUE> [--org o] <text...|->");
    }
    let text = if pa.rest.len() == 1 && pa.rest[0] == "-" {
        let mut buf = Vec::new();
        let _ = io::stdin().read_to_end(&mut buf);
        String::from_utf8_lossy(&buf)
            .trim_end_matches('\n')
            .to_string()
    } else {
        pa.rest.join(" ")
    };
    if text.is_empty() {
        return usage_err("log: empty text");
    }
    ctx.require_store()?;
    let e = ctx.resolve_entry(&pa.issue, &pa.org)?;
    journal_append(&e.dir, &text)?;
    let _ = writeln!(out, "logged {}/{}", e.org, e.issue);
    Ok(())
}

/// Split `key=value` / `key+=value`; key is `[a-z]+`.
fn parse_kv(arg: &str) -> Option<(&str, bool, &str)> {
    let b = arg.as_bytes();
    let n = b.iter().take_while(|c| c.is_ascii_lowercase()).count();
    if n == 0 {
        return None;
    }
    let (plus, p) = if b.get(n) == Some(&b'+') {
        (true, n + 1)
    } else {
        (false, n)
    };
    if b.get(p) != Some(&b'=') {
        return None;
    }
    Some((&arg[..n], plus, &arg[p + 1..]))
}

fn cmd_set(ctx: &Ctx, args: &[String], out: Out) -> R<()> {
    let pa = parse_issue_args("set", args)?;
    if pa.rest.is_empty() {
        return usage_err("usage: triage set <ISSUE> [--org o] key=value...");
    }
    ctx.require_store()?;
    let e = ctx.resolve_entry(&pa.issue, &pa.org)?;
    let state = format!("{}/state.md", e.dir);
    let mut content = io_err(read_lossy(&state), "cannot read state.md")?;
    let mut log: Vec<String> = Vec::new();
    for arg in &pa.rest {
        let Some((key, plus, val)) = parse_kv(arg) else {
            return usage_err(format!("set: expected key=value, got '{arg}'"));
        };
        if !SETTABLE.split(' ').any(|k| k == key) {
            return die(format!("key '{key}' cannot be set (allowed: {SETTABLE})"));
        }
        if plus {
            if key != "tracker" && key != "related" {
                return die("'+=' only works for tracker and related");
            }
            if val.is_empty() {
                return die(format!("set: empty value for {key}+="));
            }
        }
        let val = val.trim_matches(is_space);
        if val.contains('\n') {
            return die(format!("value for {key} must be a single line"));
        }
        let err = field_error(key, val);
        if !err.is_empty() {
            return die(err.join("\n"));
        }
        if plus {
            let cur = fm_get(&content, key);
            log.push(format!("set {key}+={val}"));
            let merged = norm_field(key, &format!("{cur}, {val}"));
            content = fm_put(&content, key, &merged);
        } else {
            let v = norm_field(key, val);
            content = fm_put(&content, key, &v);
            log.push(format!("set {key}={v}"));
        }
    }
    if fm_get(&content, "status") == "closed" && fm_get(&content, "verdict") == "unknown" {
        return die(format!(
            "cannot close {}/{} with verdict unknown; set verdict first",
            e.org, e.issue
        ));
    }
    content = fm_put(&content, "updated", &today());
    let tmp = format!("{state}.new.{}", std::process::id());
    io_err(fs::write(&tmp, content), "cannot write state.md")?;
    io_err(fs::rename(&tmp, &state), "cannot replace state.md")?;
    journal_append(&e.dir, &log.join("\n"))?;
    let _ = writeln!(out, "updated {}/{}", e.org, e.issue);
    Ok(())
}

fn cmd_show(ctx: &Ctx, args: &[String], out: Out) -> R<()> {
    let pa = parse_issue_args("show", args)?;
    if let Some(x) = pa.rest.first() {
        return usage_err(format!("show: unexpected argument: {x}"));
    }
    ctx.require_store()?;
    let e = ctx.resolve_entry(&pa.issue, &pa.org)?;
    let st = io_err(
        fs::read(format!("{}/state.md", e.dir)),
        "cannot read state.md",
    )?;
    let _ = out.write_all(&st);
    let jp = format!("{}/journal.md", e.dir);
    if is_file(&jp) {
        let j = io_err(fs::read(&jp), "cannot read journal.md")?;
        let _ = out.write_all(b"\n");
        let _ = out.write_all(&j);
    }
    Ok(())
}

fn cmd_path(ctx: &Ctx, args: &[String], out: Out) -> R<()> {
    let pa = parse_issue_args("path", args)?;
    if let Some(x) = pa.rest.first() {
        return usage_err(format!("path: unexpected argument: {x}"));
    }
    ctx.require_store()?;
    let e = ctx.resolve_entry(&pa.issue, &pa.org)?;
    let _ = writeln!(out, "{}", e.dir);
    Ok(())
}

fn cmd_list(ctx: &Ctx, args: &[String], out: Out) -> R<()> {
    let (mut org, mut status, mut verdict) = (String::new(), String::new(), String::new());
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--org" | "--status" | "--verdict" => {
                if i + 1 >= args.len() {
                    return usage_err(format!("list: {a} requires a value"));
                }
                let v = args[i + 1].clone();
                match a {
                    "--org" => org = v,
                    "--status" => status = v,
                    _ => verdict = v,
                }
                i += 2;
            }
            _ => return usage_err(format!("list: unexpected argument: {a}")),
        }
    }
    if !status.is_empty() {
        let err = field_error("status", &status);
        if !err.is_empty() {
            return die(err.join("\n"));
        }
    }
    if !verdict.is_empty() {
        let err = field_error("verdict", &verdict);
        if !err.is_empty() {
            return die(err.join("\n"));
        }
    }
    ctx.require_store()?;
    // (updated, ORG/ISSUE, status, verdict, url)
    let mut rows: Vec<(String, String, String, String, String)> = Vec::new();
    for (o, i, d) in ctx.all_dirs() {
        let f = format!("{d}/state.md");
        if !is_file(&f) || (!org.is_empty() && org != o) {
            continue;
        }
        let c = read_lossy(&f).unwrap_or_default();
        let (st, vd) = (fm_get(&c, "status"), fm_get(&c, "verdict"));
        if (!status.is_empty() && status != st) || (!verdict.is_empty() && verdict != vd) {
            continue;
        }
        rows.push((
            fm_get(&c, "updated"),
            format!("{o}/{i}"),
            st,
            vd,
            fm_get(&c, "url"),
        ));
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    for (upd, label, st, vd, url) in rows {
        if url.is_empty() {
            let _ = writeln!(out, "{label}  {st}  {vd}  {upd}");
        } else {
            let _ = writeln!(out, "{label}  {st}  {vd}  {upd}  {url}");
        }
    }
    Ok(())
}

/// Files under `dir` (recursive, hidden included, symlinks skipped), sorted.
fn walk_files(dir: &Path, rel: &str, acc: &mut Vec<(String, String)>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut ents: Vec<_> = rd.flatten().collect();
    ents.sort_by_key(|e| e.file_name());
    for e in ents {
        let name = e.file_name().to_string_lossy().into_owned();
        let rp = format!("{rel}/{name}");
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            walk_files(&e.path(), &rp, acc);
        } else if ft.is_file() {
            acc.push((rp, e.path().to_string_lossy().into_owned()));
        }
    }
}

fn grep_file(label: &str, rel: &str, path: &str, pat: &str, out: Out) {
    let Ok(bytes) = fs::read(path) else { return };
    if bytes.contains(&0) {
        return; // binary file
    }
    let text = String::from_utf8_lossy(&bytes);
    for (n, line) in lines(&text).into_iter().enumerate() {
        if line.to_lowercase().contains(pat) {
            let _ = writeln!(out, "{label}: {rel}:{}:{line}", n + 1);
        }
    }
}

fn cmd_find(ctx: &Ctx, args: &[String], out: Out) -> R<()> {
    if args.len() != 1 {
        return usage_err("usage: triage find <pattern>");
    }
    if args[0].is_empty() {
        return usage_err("find: empty pattern");
    }
    ctx.require_store()?;
    let pat = args[0].to_lowercase();
    for (o, i, d) in ctx.all_dirs() {
        if !is_file(&format!("{d}/state.md")) {
            continue;
        }
        let label = format!("{o}/{i}");
        let mut files = vec![("state.md".to_string(), format!("{d}/state.md"))];
        if is_file(&format!("{d}/journal.md")) {
            files.push(("journal.md".to_string(), format!("{d}/journal.md")));
        }
        let ev = format!("{d}/evidence");
        if is_dir(&ev) {
            walk_files(Path::new(&ev), "evidence", &mut files);
        }
        for (rel, path) in files {
            grep_file(&label, &rel, &path, &pat, out);
        }
    }
    Ok(())
}

struct Checker<'a> {
    store: &'a str,
    problems: usize,
}

impl Checker<'_> {
    fn prob(&mut self, out: Out, label: &str, msg: &str) {
        let _ = writeln!(out, "{label}: {msg}");
        self.problems += 1;
    }

    fn related_exists(&self, item: &str, org: &str) -> bool {
        let (o, issue) = match item.split_once('/') {
            Some((o, i)) => (o, i),
            None => (org, item),
        };
        is_file(&format!("{}/{o}/{}/state.md", self.store, upper(issue)))
    }

    fn entry(&mut self, out: Out, org: &str, issue: &str, dir: &str) {
        let label = format!("{org}/{issue}");
        let label = label.as_str();
        let f = format!("{dir}/state.md");
        if !valid_id(org) {
            self.prob(out, label, "invalid org directory name");
        }
        if !valid_id(issue) {
            self.prob(out, label, "invalid issue directory name");
        }
        if !is_file(&format!("{dir}/journal.md")) {
            self.prob(out, label, "journal.md is missing");
        }
        if !is_file(&f) {
            self.prob(out, label, "state.md is missing");
            return;
        }
        let c = read_lossy(&f).unwrap_or_default();
        let st = fm_struct_problems(&c);
        if !st.is_empty() {
            for l in st {
                self.prob(out, label, &l);
            }
            return;
        }
        if fm_get(&c, "schema") != "1" {
            self.prob(out, label, "schema must be 1");
        }
        let v = fm_get(&c, "issue");
        if v != issue {
            self.prob(out, label, &format!("issue '{v}' does not match directory"));
        }
        let v = fm_get(&c, "org");
        if v != org {
            self.prob(out, label, &format!("org '{v}' does not match directory"));
        }
        for key in [
            "project",
            "url",
            "status",
            "verdict",
            "confidence",
            "commit",
            "repo",
            "tracker",
            "related",
        ] {
            for m in field_error(key, &fm_get(&c, key)) {
                self.prob(out, label, &m);
            }
        }
        let created = fm_get(&c, "created");
        let updated = fm_get(&c, "updated");
        let (cok, uok) = (valid_date(&created), valid_date(&updated));
        if !cok {
            self.prob(out, label, &format!("invalid created date '{created}'"));
        }
        if !uok {
            self.prob(out, label, &format!("invalid updated date '{updated}'"));
        }
        if cok && uok && updated < created {
            self.prob(
                out,
                label,
                &format!("updated {updated} is before created {created}"),
            );
        }
        if fm_get(&c, "status") == "closed" && fm_get(&c, "verdict") == "unknown" {
            self.prob(out, label, "closed entry has verdict unknown");
        }
        for item in list_items(&fm_get(&c, "related")) {
            if !valid_rel(&item) {
                continue;
            }
            if !self.related_exists(&item, org) {
                self.prob(
                    out,
                    label,
                    &format!("related target '{item}' does not exist"),
                );
            }
        }
    }
}

fn cmd_check(ctx: &Ctx, args: &[String], out: Out) -> R<()> {
    if !args.is_empty() {
        return usage_err("usage: triage check");
    }
    ctx.require_store()?;
    let mut ck = Checker {
        store: &ctx.store,
        problems: 0,
    };
    let dirs = ctx.all_dirs();
    for (o, i, d) in &dirs {
        ck.entry(out, o, i, d);
    }
    if ck.problems > 0 {
        let _ = out.flush();
        return Err(Fail {
            code: 1,
            msg: format!(
                "triage: {} problem(s) in {} entries",
                ck.problems,
                dirs.len()
            ),
        });
    }
    let _ = writeln!(out, "OK ({} entries)", dirs.len());
    Ok(())
}

fn collect_text(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut ents: Vec<_> = rd.flatten().collect();
    ents.sort_by_key(|e| e.file_name());
    for e in ents {
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            collect_text(&e.path(), files);
        } else if ft.is_file() {
            files.push(e.path());
        }
    }
}

fn copy_tree(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for e in fs::read_dir(src)? {
        let e = e?;
        let to = dst.join(e.file_name());
        let ft = e.file_type()?;
        if ft.is_dir() {
            copy_tree(&e.path(), &to)?;
        } else if ft.is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(fs::read_link(e.path())?, &to)?;
        } else {
            fs::copy(e.path(), &to)?;
        }
    }
    Ok(())
}

fn cmd_import(ctx: &Ctx, args: &[String], out: Out) -> R<()> {
    let (mut src, mut org, mut project) = (String::new(), String::new(), String::new());
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--org" | "--project" => {
                if i + 1 >= args.len() {
                    return usage_err(format!("import: {a} requires a value"));
                }
                if a == "--org" {
                    org = args[i + 1].clone();
                } else {
                    project = args[i + 1].clone();
                }
                i += 2;
            }
            _ if a.starts_with('-') => return usage_err(format!("import: unknown option: {a}")),
            _ => {
                if !src.is_empty() {
                    return usage_err(format!("import: unexpected argument: {a}"));
                }
                src = a.to_string();
                i += 1;
            }
        }
    }
    if src.is_empty() {
        return usage_err("usage: triage import <dir> --org <org> [--project p]");
    }
    if org.is_empty() {
        return usage_err("import: --org is required");
    }
    if !valid_id(&org) {
        return die(format!("invalid org '{org}'"));
    }
    let err = field_error("project", &project);
    if !err.is_empty() {
        return die(err.join("\n"));
    }
    if !is_dir(&src) {
        return die(format!("not a directory: {src}"));
    }
    let src = io_err(fs::canonicalize(&src), "cannot resolve source")?
        .to_string_lossy()
        .into_owned();
    ctx.ensure_store()?;

    let (mut imported, mut skipped, mut ignored) = (0, 0, 0);
    for name in subdirs(&src) {
        let sub = format!("{src}/{name}");
        if !extract::issue_dir_name(&name) {
            let _ = writeln!(out, "ignored {name} (not an issue id)");
            ignored += 1;
            continue;
        }
        let issue = upper(&name);
        if fs::symlink_metadata(format!("{}/{org}/{issue}", ctx.store)).is_ok() {
            let _ = writeln!(out, "skipped {org}/{issue} (exists)");
            skipped += 1;
            continue;
        }
        let mut files = Vec::new();
        collect_text(Path::new(&sub), &mut files);
        let mut raw = Vec::new();
        for f in &files {
            if let Ok(b) = fs::read(f) {
                raw.extend_from_slice(&b);
            }
        }
        let text = String::from_utf8_lossy(&raw);
        let url = extract::sentry_url(&text).unwrap_or_default();
        let tracker = extract::tracker_urls(&text).join(", ");
        let verdict = extract::verdict(&text);
        let confidence = extract::confidence(&text);
        create_entry(
            ctx, &org, &issue, &project, &url, "", "", &tracker, verdict, confidence,
        )?;
        let dest = format!("{}/{org}/{issue}", ctx.store);
        io_err(
            copy_tree(Path::new(&sub), Path::new(&format!("{dest}/legacy"))),
            "cannot copy legacy files",
        )?;
        let mut summary = format!("imported from {sub}\nextracted: verdict={verdict}");
        if !url.is_empty() {
            summary.push_str(&format!(" url={url}"));
        }
        if !tracker.is_empty() {
            summary.push_str(&format!(" tracker={tracker}"));
        }
        if !confidence.is_empty() {
            summary.push_str(&format!(" confidence={confidence}"));
        }
        journal_append(&dest, &summary)?;
        let _ = writeln!(
            out,
            "imported {org}/{issue} (verdict={verdict}{}{})",
            if url.is_empty() { "" } else { ", url" },
            if tracker.is_empty() { "" } else { ", tracker" }
        );
        imported += 1;
    }
    let _ = writeln!(
        out,
        "import: {imported} imported, {skipped} skipped, {ignored} ignored"
    );
    Ok(())
}

fn git_out(store: &str, args: &[&str]) -> R<String> {
    let o = Command::new("git")
        .args(["-C", store, "-c", "commit.gpgsign=false"])
        .args(args)
        .stderr(Stdio::inherit())
        .output();
    match o {
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).into_owned()),
        _ => die(format!("git {} failed", args.first().unwrap_or(&""))),
    }
}

fn cmd_backup(ctx: &Ctx, args: &[String], out: Out) -> R<()> {
    let mut push = false;
    for a in args {
        if a == "--push" {
            push = true;
        } else {
            return usage_err(format!("backup: unexpected argument: {a}"));
        }
    }
    if !is_dir(&format!("{}/.git", ctx.store)) {
        return die(format!("no store at {} (run 'triage init')", ctx.store));
    }
    let s = &ctx.store;
    if git_out(s, &["status", "--porcelain", "--untracked-files=all"])?.is_empty() {
        let _ = writeln!(out, "nothing to commit");
    } else {
        git_out(s, &["add", "-A"])?;
        let msg = format!("triage: backup {}", now_utc());
        let ok = Command::new("git")
            .args([
                "-C",
                s,
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "-m",
                &msg,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .status()
            .map(|st| st.success())
            .unwrap_or(false);
        if !ok {
            return die("git commit failed (is user.name/user.email configured?)");
        }
        let _ = writeln!(out, "committed");
    }
    if push {
        let remotes = git_out(s, &["remote"]).unwrap_or_default();
        let remote = remotes.lines().next().unwrap_or("").to_string();
        if remote.is_empty() {
            return die(format!("no git remote configured in {s}"));
        }
        let ok = Command::new("git")
            .args([
                "-C",
                s,
                "-c",
                "commit.gpgsign=false",
                "push",
                "-q",
                &remote,
                "HEAD",
            ])
            .status()
            .map(|st| st.success())
            .unwrap_or(false);
        if !ok {
            return die("git push failed");
        }
        let _ = writeln!(out, "pushed to {remote}");
    }
    Ok(())
}

fn run(args: &[String], out: Out) -> R<i32> {
    let ctx = Ctx::from_env()?;
    let Some(cmd) = args.first() else {
        eprint!("{USAGE}");
        return Ok(2);
    };
    let rest = &args[1..];
    match cmd.as_str() {
        "init" => cmd_init(&ctx, rest, out)?,
        "new" => cmd_new(&ctx, rest, out)?,
        "log" => cmd_log(&ctx, rest, out)?,
        "set" => cmd_set(&ctx, rest, out)?,
        "show" => cmd_show(&ctx, rest, out)?,
        "path" => cmd_path(&ctx, rest, out)?,
        "list" => cmd_list(&ctx, rest, out)?,
        "find" => cmd_find(&ctx, rest, out)?,
        "check" => cmd_check(&ctx, rest, out)?,
        "import" => cmd_import(&ctx, rest, out)?,
        "backup" => cmd_backup(&ctx, rest, out)?,
        "--version" | "version" => {
            let _ = writeln!(out, "triage {VERSION}");
        }
        "help" | "-h" | "--help" => {
            let _ = out.write_all(USAGE.as_bytes());
        }
        other => return usage_err(format!("unknown command: {other}")),
    }
    Ok(0)
}

fn main() {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    let code = match run(&args, &mut out) {
        Ok(c) => c,
        Err(f) => {
            let _ = out.flush();
            eprintln!("{}", f.msg);
            f.code
        }
    };
    let _ = out.flush();
    std::process::exit(code);
}
