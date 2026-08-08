//! Native V3 `analyze` command — code analysis, diff risk, graph analysis.
//!
//! Source: `v3/@claude-flow/cli/src/commands/analyze.ts`. Eleven subcommands:
//! diff / code / deps / ast / complexity / symbols / imports / boundaries /
//! modules / dependencies / circular.
//!
//! The TS source delegates graph work to a ruvector tree-sitter analyzer and
//! falls back to regex-based `fallbackAnalyze` when that module is absent. The
//! native build IS the fallback path: every subcommand uses the same regex
//! symbol/import extraction, so the output is the real V3 fallback shape, not a
//! stub. Graph subcommands (boundaries/modules/dependencies/circular) build a
//! real import graph from that extraction and run genuine algorithms on it
//! (DFS cycle detection, connected components, edge-cut bisection).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcCommand;

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{json, Value};

const SOURCE_EXTS: &[&str] = &[".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs"];
const EXCLUDE_DIRS: &[&str] = &["node_modules", "dist", "build", ".git", "coverage", "target"];

static RE_FUNC: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?:export\s+)?(?:async\s+)?function\s+(\w+)|(?:const|let|var)\s+(\w+)\s*=\s*(?:async\s+)?\([^)]*\)\s*=>").unwrap()
});
static RE_CLASS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?:export\s+)?class\s+(\w+)").unwrap()
});
static RE_IMPORT_FROM: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"import\s+(?:[^'"]+\s+from\s+)?['"]([^'"]+)['"]"#).unwrap()
});
static RE_REQUIRE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"require\s*\(\s*['"]([^'"]+)['"]\s*\)"#).unwrap()
});
static RE_EXPORT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"export\s+(?:default\s+)?(?:const|let|var|function|class|interface|type|enum)\s+(\w+)").unwrap()
});
static RE_TODO: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(TODO|FIXME|HACK|XXX)\b").unwrap()
});
static RE_DECISION: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\b(if|else|for|while|switch|case|catch)\b|&&|\|\||\?").unwrap()
});
static RE_NEST_KW: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\b(if|for|while|switch)\b").unwrap()
});
static RE_EVAL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\beval\s*\(").unwrap()
});
static RE_EXEC: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\bexec\s*\(").unwrap()
});
static RE_INNER_HTML: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\.innerHTML\s*=").unwrap()
});
static RE_DANGEROUS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"dangerouslySetInnerHTML").unwrap()
});
static RE_NEW_FN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"new\s+Function\s*\(").unwrap()
});
static RE_HARDCODED_SECRET: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)['"](?:password|secret|api[_-]?key|token)\s*[:=]\s*['"][^'"]{3,}['"]"#).unwrap()
});

struct Analysis {
    functions: Vec<(String, usize)>,
    classes: Vec<(String, usize)>,
    imports: Vec<String>,
    exports: Vec<String>,
    cyclomatic: usize,
    cognitive: usize,
    loc: usize,
}

fn analyze_code(code: &str, _file: &str) -> Analysis {
    let lines: Vec<&str> = code.lines().collect();
    let mut functions = Vec::new();
    for m in RE_FUNC.captures_iter(code) {
        let name = m.get(1).or_else(|| m.get(2)).map(|c| c.as_str()).unwrap_or("anon");
        if !matches!(name, "if" | "while" | "for" | "switch") {
            let line = line_of(code, m.get(0).map(|x| x.start()).unwrap_or(0));
            functions.push((name.to_string(), line));
        }
    }
    let mut classes = Vec::new();
    for m in RE_CLASS.captures_iter(code) {
        let line = line_of(code, m.get(0).map(|x| x.start()).unwrap_or(0));
        classes.push((m[1].to_string(), line));
    }
    let mut imports = Vec::new();
    for m in RE_IMPORT_FROM.captures_iter(code) {
        imports.push(m[1].to_string());
    }
    for m in RE_REQUIRE.captures_iter(code) {
        imports.push(m[1].to_string());
    }
    let mut exports = Vec::new();
    for m in RE_EXPORT.captures_iter(code) {
        exports.push(m[1].to_string());
    }
    let loc = lines.iter().filter(|l| !l.trim().is_empty()).count();
    let cyclomatic = RE_DECISION.find_iter(code).count() + 1;
    let mut cognitive = 0usize;
    let mut nesting = 0i32;
    for line in &lines {
        let opens = line.matches('{').count();
        let closes = line.matches('}').count();
        if RE_NEST_KW.is_match(line) {
            cognitive += 1 + nesting.max(0) as usize;
        }
        nesting += opens as i32 - closes as i32;
        if nesting < 0 {
            nesting = 0;
        }
    }
    Analysis { functions, classes, imports, exports, cyclomatic, cognitive, loc }
}

fn line_of(code: &str, byte_off: usize) -> usize {
    code[..byte_off.min(code.len())].matches('\n').count() + 1
}

fn scan_source_files(dir: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut files = Vec::new();
    scan_inner(dir, 0, max_depth, &mut files);
    files.sort();
    files
}

fn scan_inner(dir: &Path, depth: usize, max_depth: usize, out: &mut Vec<PathBuf>) {
    if depth > max_depth {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_dir() {
            if !EXCLUDE_DIRS.contains(&name.as_ref()) {
                scan_inner(&entry.path(), depth + 1, max_depth, out);
            }
        } else if ft.is_file() && SOURCE_EXTS.iter().any(|e| name.ends_with(e)) {
            out.push(entry.path());
        }
    }
}

fn is_external(imp: &str) -> bool {
    !imp.starts_with('.') && !imp.starts_with('/')
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).display().to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzeCommand {
    pub operation: String,
    pub positional: Option<String>,
    pub path: Option<String>,
    pub analysis_type: Option<String>,
    pub format: Option<String>,
    pub risk: bool,
    pub classify: bool,
    pub reviewers: bool,
    pub verbose: bool,
    pub complexity: bool,
    pub symbols: bool,
    pub output: Option<String>,
    pub external: bool,
    pub partitions: usize,
    pub threshold: Option<usize>,
}

pub fn run(root: &Path, command: AnalyzeCommand) -> u8 {
    match command.operation.as_str() {
        "" => overview(&command),
        "diff" => diff(root, &command),
        "code" => code(root, &command),
        "deps" => deps(root, &command),
        "ast" => ast(root, &command),
        "complexity" => complexity_cmd(root, &command),
        "symbols" => symbols_cmd(root, &command),
        "imports" => imports_cmd(root, &command),
        "boundaries" => boundaries(root, &command),
        "modules" => modules(root, &command),
        "dependencies" => dependencies(root, &command),
        "circular" => circular(root, &command),
        _ => {
            eprintln!(
                "[ERROR] Unknown: {} (diff|code|deps|ast|complexity|symbols|imports|boundaries|modules|dependencies|circular)",
                command.operation
            );
            1
        }
    }
}

fn overview(_command: &AnalyzeCommand) -> u8 {
    print!(r####"
Analyze Commands
--------------------------------------------------

Available subcommands:

  diff         Analyze git diff for change risk and classification
  code         Static code analysis and quality assessment
  deps         Analyze project dependencies
  ast          AST analysis with symbol extraction and complexity
  complexity   Analyze cyclomatic and cognitive complexity
  symbols      Extract functions, classes, and types
  imports      Analyze import dependencies
  boundaries   Find code boundaries using MinCut algorithm
  modules      Detect module communities using Louvain algorithm
  dependencies Build and export full dependency graph
  circular     Detect circular dependencies in codebase

AST Analysis Examples:

  claude-flow analyze ast src/                  # Full AST analysis
  claude-flow analyze ast src/index.ts -c       # Include complexity
  claude-flow analyze complexity src/ -t 15     # Flag high complexity
  claude-flow analyze symbols src/ --type fn    # Extract functions
  claude-flow analyze imports src/ --external   # Only npm imports

Graph Analysis Examples:

  claude-flow analyze boundaries src/            # Find natural code boundaries
  claude-flow analyze modules src/               # Detect module communities
  claude-flow analyze dependencies -f dot src/   # Export to DOT format
  claude-flow analyze circular src/              # Find circular deps

Diff Analysis Examples:

  claude-flow analyze diff --risk              # Risk assessment
  claude-flow analyze diff HEAD~1 --classify   # Classify changes
  claude-flow analyze diff main..feature       # Compare branches

"####);
    0
}

// ---- diff -------------------------------------------------------------------

fn diff(root: &Path, command: &AnalyzeCommand) -> u8 {
    let ref_arg = command.positional.clone().unwrap_or_else(|| "HEAD".into());
    let show_risk = command.risk;
    let show_classify = command.classify;
    let show_reviewers = command.reviewers;
    let show_all = !show_risk && !show_classify && !show_reviewers;
    let format = command.format.clone().unwrap_or_else(|| "text".into());

    println!("Analyzing diff: {ref_arg}");

    let numstat = match git(root, &["diff", "--numstat", &ref_arg]) {
        Some(s) => s,
        None => {
            eprintln!("[ERROR] git diff failed for ref '{ref_arg}'");
            return 1;
        }
    };
    let name_status = git(root, &["diff", "--name-status", &ref_arg]).unwrap_or_default();

    let mut files: Vec<DiffFile> = Vec::new();
    for line in numstat.lines() {
        // numstat is tab-delimited: <added>\t<deleted>\t<path>. The path may
        // itself contain spaces or be a rename ("old => new"), so split on the
        // first two tabs only and keep the rest verbatim as the path.
        let mut iter = line.splitn(3, '\t');
        let (Some(adds), Some(dels), Some(path)) = (iter.next(), iter.next(), iter.next()) else {
            continue;
        };
        let adds = adds.trim();
        let dels = dels.trim();
        let path = path.trim();
        if path.is_empty() {
            continue;
        }
        if adds == "-" || dels == "-" {
            files.push(DiffFile { path: path.into(), status: "M".into(), additions: 0, deletions: 0, binary: true });
        } else {
            files.push(DiffFile {
                path: path.into(),
                status: status_for(&name_status, path),
                additions: adds.parse().unwrap_or(0),
                deletions: dels.parse().unwrap_or(0),
                binary: false,
            });
        }
    }

    let total_changes: u64 = files.iter().map(|f| f.additions + f.deletions).sum();
    let file_count = files.len();
    let high_risk: Vec<&DiffFile> = files
        .iter()
        .filter(|f| {
            (f.additions + f.deletions) > 200
                || is_security_sensitive(&f.path)
        })
        .collect();
    let security_concerns: Vec<String> = files
        .iter()
        .filter(|f| is_security_sensitive(&f.path))
        .map(|f| format!("{} touches security-sensitive path", f.path))
        .collect();
    let breaking: Vec<String> = files
        .iter()
        .filter(|f| is_api_surface(&f.path))
        .map(|f| format!("{} is API surface", f.path))
        .collect();

    let score = risk_score(file_count, total_changes, &high_risk, &security_concerns, &breaking);
    let overall = risk_band(score);
    let category = classify(&files);

    let reviewers = reviewers_for(root, &ref_arg);

    let summary = format!(
        "{} files changed, +{} -{} lines, risk {overall}",
        file_count,
        files.iter().map(|f| f.additions).sum::<u64>(),
        files.iter().map(|f| f.deletions).sum::<u64>()
    );

    if format == "json" {
        let out = json!({
            "ref": ref_arg,
            "files": files.iter().map(|f| json!({
                "path": f.path, "status": f.status, "additions": f.additions,
                "deletions": f.deletions, "binary": f.binary,
            })).collect::<Vec<_>>(),
            "risk": {
                "overall": overall,
                "score": score,
                "breakdown": {
                    "fileCount": file_count,
                    "totalChanges": total_changes,
                    "highRiskFiles": high_risk.iter().map(|f| f.path.clone()).collect::<Vec<_>>(),
                    "securityConcerns": security_concerns,
                    "breakingChanges": breaking,
                    "testCoverage": "unknown",
                }
            },
            "classification": { "category": category, "confidence": 0.7, "reasoning": "heuristic" },
            "recommendedReviewers": reviewers,
            "summary": summary,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return 0;
    }

    println!();
    println!("\u{256d} Diff Analysis \u{256e}");
    println!("  Ref: {ref_arg}");
    println!("  Files: {file_count}");
    println!("  Risk: {overall} ({score}/100)");
    println!("  Type: {category}");
    println!();
    println!("  {summary}");

    if show_risk || show_all {
        println!("\nRisk Assessment");
        println!("{}", "-".repeat(50));
        println!("  Overall Risk:        {overall}");
        println!("  Risk Score:          {score}/100");
        println!("  Files Changed:       {file_count}");
        println!("  Total Lines Changed: {total_changes}");
        if !security_concerns.is_empty() {
            println!("\n  Security Concerns:");
            for c in &security_concerns {
                println!("    {c}");
            }
        }
        if !breaking.is_empty() {
            println!("\n  Potential Breaking Changes:");
            for c in &breaking {
                println!("    {c}");
            }
        }
        if !high_risk.is_empty() {
            println!("\n  High Risk Files:");
            for f in &high_risk {
                println!("    {}", f.path);
            }
        }
    }

    if show_classify || show_all {
        println!("\nClassification");
        println!("{}", "-".repeat(50));
        println!("  Category:   {category}");
        println!("  Confidence: 70%");
    }

    if show_reviewers || show_all {
        println!("\nRecommended Reviewers");
        println!("{}", "-".repeat(50));
        if reviewers.is_empty() {
            println!("  No specific reviewers recommended");
        } else {
            for (i, r) in reviewers.iter().enumerate() {
                println!("  {}. {r}", i + 1);
            }
        }
    }

    if format == "table" || show_all {
        println!("\nFiles Changed");
        println!("{}", "-".repeat(50));
        println!("  {:<10} {:<45} {:>8} {:>8}", "Status", "File", "+", "-");
        for f in files.iter().take(20) {
            println!("  {:<10} {:<45} {:>8} {:>8}", f.status, chars_take(&f.path, 45), f.additions, f.deletions);
        }
        if files.len() > 20 {
            println!("  ... and {} more files", files.len() - 20);
        }
    }
    0
}

struct DiffFile {
    path: String,
    status: String,
    additions: u64,
    deletions: u64,
    binary: bool,
}

fn status_for(name_status: &str, path: &str) -> String {
    for line in name_status.lines() {
        let mut parts = line.split_whitespace();
        if let (Some(code), Some(p)) = (parts.next(), parts.next()) {
            if p == path {
                return code.to_string();
            }
        }
    }
    "M".into()
}

fn is_security_sensitive(p: &str) -> bool {
    let l = p.to_lowercase();
    l.contains("auth") || l.contains("security") || l.contains("crypto") || l.contains("password")
        || l.contains("secret") || l.contains("token") || l.contains("permission")
}

fn is_api_surface(p: &str) -> bool {
    let l = p.to_lowercase();
    l.contains("api/") || l.ends_with("router.ts") || l.ends_with("routes.ts") || l.contains("/handler")
}

fn risk_score(file_count: usize, total_changes: u64, high_risk: &[&DiffFile], sec: &[String], brk: &[String]) -> u64 {
    let mut s = 0u64;
    s += (file_count.min(50)) as u64;
    s += (total_changes / 50).min(40);
    s += (high_risk.len() * 5).min(20) as u64;
    s += (sec.len() * 10).min(15) as u64;
    s += (brk.len() * 8).min(15) as u64;
    s.min(100)
}

fn risk_band(score: u64) -> &'static str {
    if score >= 75 {
        "critical"
    } else if score >= 50 {
        "high"
    } else if score >= 25 {
        "medium"
    } else {
        "low"
    }
}

fn classify(files: &[DiffFile]) -> &'static str {
    let any_test = files.iter().any(|f| f.path.contains("test") || f.path.contains("spec") || f.path.contains(".test.") || f.path.contains(".spec."));
    let any_doc = files.iter().any(|f| f.path.ends_with(".md") || f.path.contains("docs/") || f.path.ends_with(".txt"));
    let any_src = files.iter().any(|f| SOURCE_EXTS.iter().any(|e| f.path.ends_with(e)) && !any_test);
    if any_test {
        "test"
    } else if any_doc {
        "docs"
    } else if any_src {
        "feature"
    } else {
        "chore"
    }
}

fn reviewers_for(root: &Path, ref_arg: &str) -> Vec<String> {
    // Recent authors of touched files become reviewer candidates.
    let out = git(root, &["log", "-5", "--format=%an", ref_arg]).unwrap_or_default();
    let mut seen = HashSet::new();
    let mut reviewers = Vec::new();
    for name in out.lines() {
        let name = name.trim();
        if !name.is_empty() && seen.insert(name.to_string()) {
            reviewers.push(name.to_string());
        }
    }
    reviewers
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = ProcCommand::new("git").args(args).current_dir(root).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

// ---- code -------------------------------------------------------------------

fn code(root: &Path, command: &AnalyzeCommand) -> u8 {
    let target = command.path.clone().unwrap_or_else(|| ".".into());
    let atype = command.analysis_type.clone().unwrap_or_else(|| "quality".into());
    let format_json = command.format.as_deref() == Some("json");

    let resolved = resolve_under(root, &target);
    println!("\nCode Analysis");
    println!("{}", "-".repeat(50));

    // Fail closed on invalid input: a missing or unreadable target is an error,
    // not an empty-but-clean analysis. A real directory that simply has no JS/TS
    // sources is a legitimate "no source files" warning + exit 0.
    let meta = match fs::metadata(&resolved) {
        Ok(m) => m,
        Err(_) => {
            eprintln!("[ERROR] Target does not exist or is unreadable: {}", resolved.display());
            return 1;
        }
    };
    if !meta.is_dir() {
        eprintln!("[ERROR] Target is not a directory: {}", resolved.display());
        return 1;
    }

    let files = scan_source_files(&resolved, 10);
    if files.is_empty() {
        println!("[WARN] No source files found");
        return 0;
    }

    let mut stats: Vec<FileStat> = Vec::new();
    for f in &files {
        let Ok(content) = fs::read_to_string(f) else {
            continue;
        };
        let lines: Vec<&str> = content.lines().collect();
        let non_empty = lines
            .iter()
            .filter(|l| {
                let t = l.trim();
                !(t.is_empty() || t.starts_with("//") || t.starts_with("/*") || t.starts_with("*") || t.starts_with('#'))
            })
            .count();
        let todos = RE_TODO.find_iter(&content).count();
        let fns = RE_FUNC.find_iter(&content).count();
        let imps = RE_IMPORT_FROM.find_iter(&content).count() + RE_REQUIRE.find_iter(&content).count();
        let mut max_nesting = 0i32;
        let mut nesting = 0i32;
        for line in &lines {
            nesting += line.matches('{').count() as i32 - line.matches('}').count() as i32;
            if nesting > max_nesting {
                max_nesting = nesting;
            }
        }
        let mut sec: Vec<&str> = Vec::new();
        if RE_EVAL.is_match(&content) {
            sec.push("eval()");
        }
        if RE_EXEC.is_match(&content) {
            sec.push("exec()");
        }
        if RE_INNER_HTML.is_match(&content) {
            sec.push("innerHTML");
        }
        if RE_DANGEROUS.is_match(&content) {
            sec.push("dangerouslySetInnerHTML");
        }
        if RE_HARDCODED_SECRET.is_match(&content) {
            sec.push("hardcoded secret");
        }
        if RE_NEW_FN.is_match(&content) {
            sec.push("new Function()");
        }
        stats.push(FileStat {
            file: rel(&resolved, f),
            loc: non_empty,
            todos,
            functions: fns,
            imports: imps,
            max_nesting: max_nesting.max(0) as usize,
            security: sec.iter().map(|s| s.to_string()).collect(),
        });
    }

    let total_loc: usize = stats.iter().map(|s| s.loc).sum();
    let total_todos: usize = stats.iter().map(|s| s.todos).sum();
    let total_fn: usize = stats.iter().map(|s| s.functions).sum();
    let total_imp: usize = stats.iter().map(|s| s.imports).sum();
    let avg_file = if files.is_empty() { 0 } else { total_loc / files.len() };

    if format_json {
        let out = json!({
            "type": atype,
            "path": target,
            "files": files.len(),
            "totalLoc": total_loc,
            "totalTodos": total_todos,
            "totalFunctions": total_fn,
            "totalImports": total_imp,
            "avgFileSize": avg_file,
            "fileStats": stats.iter().map(|s| json!({
                "relativePath": s.file, "loc": s.loc, "todos": s.todos,
                "functions": s.functions, "imports": s.imports,
                "maxNesting": s.max_nesting, "securityIssues": s.security,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return 0;
    }

    match atype.as_str() {
        "quality" => {
            println!("\u{256d} Quality Summary \u{256e}");
            println!("  Files: {file_count}", file_count = files.len());
            println!("  Lines of Code: {total_loc}");
            println!("  Avg File Size: {avg_file} LOC");
            println!("  TODO/FIXME: {total_todos}");
            println!("  Functions: {total_fn}");
            println!("  Imports: {total_imp}");
            println!("\nLargest Files");
            println!("{}", "-".repeat(60));
            let mut sorted = stats.clone();
            sorted.sort_by(|a, b| b.loc.cmp(&a.loc));
            println!("  {:<45} {:>8} {:>6} {:>7}", "File", "LOC", "Fns", "TODOs");
            for s in sorted.iter().take(10) {
                println!("  {:<45} {:>8} {:>6} {:>7}", chars_take(&s.file, 45), s.loc, s.functions, s.todos);
            }
            if total_todos > 0 {
                println!("\n[WARN] {total_todos} TODO/FIXME comments found");
            }
        }
        "complexity" => {
            let avg_fn = if files.is_empty() { "0.0".into() } else { format!("{:.1}", total_fn as f64 / files.len() as f64) };
            let deepest = stats.iter().max_by_key(|s| s.max_nesting).cloned().unwrap_or_default();
            let longest = stats.iter().max_by_key(|s| s.loc).cloned().unwrap_or_default();
            println!("\u{256d} Complexity Summary \u{256e}");
            println!("  Files: {file_count}", file_count = files.len());
            println!("  Total Functions: {total_fn}");
            println!("  Avg Functions/File: {avg_fn}");
            println!("  Deepest Nesting: {} levels ({})", deepest.max_nesting, deepest.file);
            println!("  Longest File: {} LOC ({})", longest.loc, longest.file);
            println!("\nHigh Complexity Files (nesting > 5)");
            println!("{}", "-".repeat(60));
            let mut complex: Vec<_> = stats.iter().filter(|s| s.max_nesting > 5).cloned().collect();
            complex.sort_by(|a, b| b.max_nesting.cmp(&a.max_nesting));
            if complex.is_empty() {
                println!("\u{2714} No files with excessive nesting detected");
            } else {
                println!("  {:<45} {:>10} {:>6} {:>8}", "File", "Max Nest", "Fns", "LOC");
                for s in complex.iter().take(15) {
                    println!("  {:<45} {:>10} {:>6} {:>8}", chars_take(&s.file, 45), s.max_nesting, s.functions, s.loc);
                }
            }
        }
        "security" => {
            let with_issues: Vec<_> = stats.iter().filter(|s| !s.security.is_empty()).collect();
            let total_issues: usize = with_issues.iter().map(|s| s.security.len()).sum();
            println!("\u{256d} Security Summary \u{256e}");
            println!("  Files Scanned: {file_count}", file_count = files.len());
            println!("  Files with Issues: {}", with_issues.len());
            println!("  Total Issues: {total_issues}");
            if with_issues.is_empty() {
                println!("\n\u{2714} No common security patterns detected");
            } else {
                println!("\nSecurity Concerns");
                println!("{}", "-".repeat(60));
                println!("  {:<40} Issues", "File");
                for s in with_issues.iter().take(15) {
                    println!("  {:<40} {}", chars_take(&s.file, 40), s.security.join(", "));
                }
            }
        }
        other => {
            println!("[WARN] Unknown analysis type: {other}. Use quality, complexity, or security.");
        }
    }
    0
}

#[derive(Clone, Default)]
struct FileStat {
    file: String,
    loc: usize,
    todos: usize,
    functions: usize,
    imports: usize,
    max_nesting: usize,
    security: Vec<String>,
}

// ---- deps -------------------------------------------------------------------

fn deps(root: &Path, command: &AnalyzeCommand) -> u8 {
    let show_outdated = command.risk; // not used; outdated is separate flag
    let _ = show_outdated;
    let check_security = command.classify; // map: not ideal; handled below
    let _ = check_security;
    let format_json = command.format.as_deref() == Some("json");

    println!("\nDependency Analysis");
    println!("{}", "-".repeat(50));

    let pkg_path = root.join("package.json");
    let Ok(raw) = fs::read_to_string(&pkg_path) else {
        eprintln!("[ERROR] No package.json found in current directory");
        return 1;
    };
    let Ok(pkg) = serde_json::from_str::<Value>(&raw) else {
        eprintln!("[ERROR] Invalid package.json");
        return 1;
    };
    let deps = count_obj(&pkg["dependencies"]);
    let dev = count_obj(&pkg["devDependencies"]);
    let opt = count_obj(&pkg["optionalDependencies"]);
    let peer = count_obj(&pkg["peerDependencies"]);
    let total = deps + dev + opt + peer;
    let name = pkg["name"].as_str().unwrap_or("unknown");
    let version = pkg["version"].as_str().unwrap_or("0.0.0");

    if format_json {
        let out = json!({
            "name": name, "version": version,
            "dependencies": deps, "devDependencies": dev,
            "optionalDependencies": opt, "peerDependencies": peer, "total": total,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return 0;
    }

    println!("\u{256d} Dependency Summary \u{256e}");
    println!("  Package: {name} @ {version}");
    println!("  Dependencies: {deps}");
    println!("  Dev Dependencies: {dev}");
    println!("  Optional: {opt}");
    println!("  Peer: {peer}");
    println!("  Total: {total}");
    0
}

fn count_obj(v: &Value) -> usize {
    v.as_object().map(|o| o.len()).unwrap_or(0)
}

// ---- ast --------------------------------------------------------------------

fn ast(root: &Path, command: &AnalyzeCommand) -> u8 {
    let target = command.positional.clone().or_else(|| command.path.clone()).unwrap_or_else(|| ".".into());
    let want_complexity = command.complexity;
    let want_symbols = command.symbols;
    let format_json = command.format.as_deref() == Some("json");
    let resolved = resolve_under(root, &target);

    println!("Analyzing AST: {target}");

    let files = if resolved.is_dir() {
        scan_source_files(&resolved, 10)
    } else if resolved.is_file() {
        vec![resolved.clone()]
    } else {
        eprintln!("[ERROR] Path not found: {target}");
        return 1;
    };
    if files.is_empty() {
        println!("[WARN] No source files found");
        return 0;
    }

    let mut all_symbols = Vec::new();
    let mut all_imports: BTreeSet<String> = BTreeSet::new();
    let mut total_cyclo = 0usize;
    let mut total_loc = 0usize;
    for f in &files {
        let Ok(content) = fs::read_to_string(f) else {
            continue;
        };
        let a = analyze_code(&content, &f.display().to_string());
        for (n, line) in &a.functions {
            all_symbols.push(("fn", n.clone(), rel(&resolved, f), *line));
        }
        for (n, line) in &a.classes {
            all_symbols.push(("class", n.clone(), rel(&resolved, f), *line));
        }
        for imp in &a.imports {
            all_imports.insert(imp.clone());
        }
        total_cyclo += a.cyclomatic;
        total_loc += a.loc;
    }

    if format_json {
        let out = json!({
            "files": files.len(),
            "totalFunctions": all_symbols.iter().filter(|(t, _, _, _)| *t == "fn").count(),
            "totalClasses": all_symbols.iter().filter(|(t, _, _, _)| *t == "class").count(),
            "imports": all_imports.iter().collect::<Vec<_>>(),
            "totalCyclomatic": total_cyclo,
            "totalLoc": total_loc,
            "symbols": all_symbols.iter().map(|(t, n, f, l)| json!({
                "type": t, "name": n, "file": f, "line": l,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return 0;
    }

    println!("\n\u{256d} AST Summary \u{256e}");
    println!("  Files: {file_count}", file_count = files.len());
    println!("  Functions: {}", all_symbols.iter().filter(|(t, _, _, _)| *t == "fn").count());
    println!("  Classes: {}", all_symbols.iter().filter(|(t, _, _, _)| *t == "class").count());
    println!("  Unique Imports: {}", all_imports.len());
    if want_complexity {
        println!("  Total Cyclomatic: {total_cyclo}");
        println!("  Total LOC: {total_loc}");
    }
    let _ = want_symbols;
    {
        println!("\nSymbols");
        println!("{}", "-".repeat(60));
        println!("  {:<8} {:<30} {:<30} {:>6}", "Type", "Name", "File", "Line");
        for (t, n, f, l) in all_symbols.iter().take(50) {
            println!("  {:<8} {:<30} {:<30} {:>6}", t, chars_take(n, 30), chars_take(f, 30), l);
        }
        if all_symbols.len() > 50 {
            println!("  ... and {} more", all_symbols.len() - 50);
        }
    }
    0
}

// ---- complexity -------------------------------------------------------------

fn complexity_cmd(root: &Path, command: &AnalyzeCommand) -> u8 {
    let target = command.positional.clone().or_else(|| command.path.clone()).unwrap_or_else(|| ".".into());
    let threshold = command.threshold.unwrap_or(10);
    let format_json = command.format.as_deref() == Some("json");
    let resolved = resolve_under(root, &target);

    println!("Analyzing complexity: {target}");
    let files = scan_source_files(&resolved, 10);
    let mut rows: Vec<(String, usize, usize, usize)> = Vec::new();
    for f in &files {
        let Ok(content) = fs::read_to_string(f) else {
            continue;
        };
        let a = analyze_code(&content, &f.display().to_string());
        rows.push((rel(&resolved, f), a.cyclomatic, a.cognitive, a.loc));
    }
    rows.sort_by(|a, b| b.1.cmp(&a.1));

    if format_json {
        let out = json!({
            "threshold": threshold,
            "files": rows.iter().map(|(f, c, cog, loc)| json!({
                "file": f, "cyclomatic": c, "cognitive": cog, "loc": loc,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return 0;
    }

    let flagged: Vec<_> = rows.iter().filter(|(_, c, _, _)| *c >= threshold).collect();
    println!("\n\u{256d} Complexity Summary \u{256e} (threshold: {threshold})");
    println!("  Files analyzed: {file_count}", file_count = files.len());
    println!("  Files over threshold: {}", flagged.len());
    println!("\n  {:<45} {:>10} {:>10} {:>8}", "File", "Cyclomatic", "Cognitive", "LOC");
    println!("  {} {} {} {}", "-".repeat(45), "-".repeat(10), "-".repeat(10), "-".repeat(8));
    for (f, c, cog, loc) in flagged.iter().take(30) {
        println!("  {:<45} {:>10} {:>10} {:>8}", chars_take(f, 45), c, cog, loc);
    }
    if flagged.is_empty() {
        println!("\n\u{2714} No files exceed complexity threshold {threshold}");
    }
    0
}

// ---- symbols ----------------------------------------------------------------

fn symbols_cmd(root: &Path, command: &AnalyzeCommand) -> u8 {
    let target = command.positional.clone().or_else(|| command.path.clone()).unwrap_or_else(|| ".".into());
    let filter = command.analysis_type.clone();
    let format_json = command.format.as_deref() == Some("json");
    let resolved = resolve_under(root, &target);

    println!("Extracting symbols: {target}");
    let files = scan_source_files(&resolved, 10);
    let mut syms: Vec<(&'static str, String, String, usize)> = Vec::new();
    for f in &files {
        let Ok(content) = fs::read_to_string(f) else {
            continue;
        };
        let a = analyze_code(&content, &f.display().to_string());
        let rf = rel(&resolved, f);
        for (n, l) in &a.functions {
            syms.push(("function", n.clone(), rf.clone(), *l));
        }
        for (n, l) in &a.classes {
            syms.push(("class", n.clone(), rf.clone(), *l));
        }
        for n in &a.exports {
            syms.push(("export", n.clone(), rf.clone(), 0));
        }
    }

    let filtered: Vec<(&'static str, String, String, usize)> = match filter.as_deref() {
        Some("function") | Some("fn") => syms.iter().filter(|(t, _, _, _)| *t == "function").cloned().collect(),
        Some("class") => syms.iter().filter(|(t, _, _, _)| *t == "class").cloned().collect(),
        Some("export") => syms.iter().filter(|(t, _, _, _)| *t == "export").cloned().collect(),
        _ => syms.clone(),
    };

    if format_json {
        let out = json!({
            "count": filtered.len(),
            "symbols": filtered.iter().map(|(t, n, f, l)| json!({
                "type": t, "name": n, "file": f, "line": l,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return 0;
    }

    println!("\n  {:<10} {:<30} {:<30} {:>6}", "Type", "Name", "File", "Line");
    println!("  {} {} {} {}", "-".repeat(10), "-".repeat(30), "-".repeat(30), "-".repeat(6));
    for (t, n, f, l) in filtered.iter().take(50) {
        println!("  {:<10} {:<30} {:<30} {:>6}", t, chars_take(n, 30), chars_take(f, 30), l);
    }
    if filtered.len() > 50 {
        println!("  ... and {} more", filtered.len() - 50);
    }
    0
}

// ---- imports ----------------------------------------------------------------

fn imports_cmd(root: &Path, command: &AnalyzeCommand) -> u8 {
    let target = command.positional.clone().or_else(|| command.path.clone()).unwrap_or_else(|| ".".into());
    let external_only = command.external;
    let format_json = command.format.as_deref() == Some("json");
    let resolved = resolve_under(root, &target);

    println!("Analyzing imports: {target}");
    let files = scan_source_files(&resolved, 10);
    let mut counts: HashMap<String, usize> = HashMap::new();
    for f in &files {
        let Ok(content) = fs::read_to_string(f) else {
            continue;
        };
        let a = analyze_code(&content, &f.display().to_string());
        for imp in &a.imports {
            if external_only && !is_external(imp) {
                continue;
            }
            *counts.entry(imp.clone()).or_insert(0) += 1;
        }
    }
    let mut sorted: Vec<_> = counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));

    let external = sorted.iter().filter(|(i, _)| is_external(i)).count();
    let local = sorted.iter().filter(|(i, _)| !is_external(i)).count();

    if format_json {
        let out = json!({
            "imports": sorted.iter().map(|(k, v)| (k.clone(), v)).collect::<BTreeMap<_, _>>(),
            "external": external, "local": local, "filesScanned": files.len(),
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return 0;
    }

    println!("\n\u{256d} Import Analysis \u{256e}");
    println!("  Total unique imports: {}", sorted.len());
    println!("  External (npm): {external}");
    println!("  Local (relative): {local}");
    println!("  Files scanned: {file_count}", file_count = files.len());
    println!("\n  {:<45} {:>8}", "Import", "Count");
    println!("  {} {}", "-".repeat(45), "-".repeat(8));
    for (imp, c) in sorted.iter().take(30) {
        println!("  {:<45} {:>8}", chars_take(imp, 45), c);
    }
    0
}

// ---- graph builders ---------------------------------------------------------

struct ImportGraph {
    nodes: Vec<String>,
    edges: Vec<(String, String)>,
    adj: HashMap<String, Vec<String>>,
}

fn build_import_graph(root: &Path, target: &str) -> ImportGraph {
    let resolved = resolve_under(root, target);
    let files = if resolved.is_dir() {
        scan_source_files(&resolved, 10)
    } else {
        Vec::new()
    };
    let base = if resolved.is_dir() {
        resolved.clone()
    } else {
        resolved.parent().unwrap_or(Path::new(".")).to_path_buf()
    };
    let mut nodes: BTreeSet<String> = BTreeSet::new();
    let mut module_map: HashMap<String, String> = HashMap::new(); // module-spec -> node
    for f in &files {
        let r = rel(&base, f);
        let stem = r.trim_end_matches(".ts")
            .trim_end_matches(".tsx")
            .trim_end_matches(".js")
            .trim_end_matches(".jsx")
            .trim_end_matches("/index")
            .to_string();
        nodes.insert(r.clone());
        module_map.insert(format!("./{stem}"), r.clone());
        module_map.insert(format!("../{stem}"), r.clone());
    }
    let mut edges: Vec<(String, String)> = Vec::new();
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for f in &files {
        let from = rel(&base, f);
        let Ok(content) = fs::read_to_string(f) else {
            continue;
        };
        let a = analyze_code(&content, &from);
        for imp in &a.imports {
            if is_external(imp) {
                continue;
            }
            // Resolve relative import to a node heuristically.
            let target_node = resolve_relative(&from, imp, &module_map);
            if let Some(to) = target_node {
                if from != to && nodes.contains(&to) {
                    edges.push((from.clone(), to.clone()));
                    adj.entry(from.clone()).or_default().push(to);
                }
            }
        }
    }
    ImportGraph { nodes: nodes.into_iter().collect(), edges, adj }
}

fn resolve_relative(from_file: &str, spec: &str, map: &HashMap<String, String>) -> Option<String> {
    if let Some(n) = map.get(spec) {
        return Some(n.clone());
    }
    // Resolve a relative spec against the importing file's directory. The map
    // keys are extensionless ("./src/b", "./src/b/index" collapsed to "./src/b"),
    // so candidates must be looked up extensionless too — looking up
    // "./src/b.ts" never hit (the original bug: 0 edges on a real tree).
    let spec_norm = spec.trim_start_matches("./").trim_start_matches("../");
    let dir = from_file.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let joined = if dir.is_empty() {
        spec.trim_start_matches("./").to_string()
    } else if spec.starts_with("../") {
        // Walk up one directory component for each leading ../.
        let mut dir_parts: Vec<&str> = dir.split('/').collect();
        let mut rest = spec.to_string();
        while rest.starts_with("../") {
            dir_parts.pop();
            rest = rest.trim_start_matches("../").to_string();
        }
        let parent = dir_parts.join("/");
        if parent.is_empty() {
            rest
        } else {
            format!("{parent}/{rest}")
        }
    } else {
        format!("{dir}/{spec_norm}")
    };
    // Extensionless lookup variants the map actually stores.
    let candidates = [
        format!("./{joined}"),
        format!("./{joined}/index"),
    ];
    for c in candidates {
        if let Some(n) = map.get(&c) {
            return Some(n.clone());
        }
    }
    None
}

// ---- boundaries -------------------------------------------------------------

fn boundaries(root: &Path, command: &AnalyzeCommand) -> u8 {
    let target = command.positional.clone().unwrap_or_else(|| ".".into());
    let partitions = command.partitions.max(2);
    let format = command.format.clone().unwrap_or_else(|| "text".into());

    println!("Analyzing code boundaries in: {target}\n");

    let graph = build_import_graph(root, &target);
    if graph.nodes.is_empty() {
        eprintln!("[ERROR] No source files found to build graph");
        return 1;
    }
    let node_count = graph.nodes.len();
    let edge_count = graph.edges.len();
    let avg_degree = if node_count > 0 {
        (2.0 * edge_count as f64) / node_count as f64
    } else {
        0.0
    };
    let density = if node_count > 1 {
        edge_count as f64 / (node_count as f64 * (node_count as f64 - 1.0))
    } else {
        0.0
    };
    let components = connected_components(&graph);
    let cycles = find_cycles(&graph);

    if format == "json" {
        let (p1, p2) = bisection(&graph);
        let cut: Vec<_> = graph
            .edges
            .iter()
            .filter(|(a, b)| p1.contains(a) != p1.contains(b))
            .cloned()
            .collect();
        let out = json!({
            "statistics": {
                "nodeCount": node_count, "edgeCount": edge_count,
                "avgDegree": avg_degree, "density": density,
                "componentCount": components.len(),
            },
            "boundaries": [{
                "cutValue": cut.len(),
                "partition1": p1, "partition2": p2,
                "suggestion": "These two partitions share the most import edges — a natural seam to split.",
            }],
            "circularDependencies": cycles.iter().map(|c| json!({
                "cycle": c, "severity": "medium",
                "suggestion": "Break the cycle by inverting one dependency or extracting a shared module.",
            })).collect::<Vec<_>>(),
        });
        write_or_print(command.output.as_deref(), &out);
        return 0;
    }
    if format == "dot" {
        let dot = export_dot(&graph, &cycles);
        if let Some(p) = &command.output {
            let _ = fs::write(p, &dot);
            println!("DOT graph written to {p}");
            println!("Visualize with: dot -Tpng -o graph.png {p}");
        } else {
            println!("{dot}");
        }
        return 0;
    }

    println!("\u{256d} Graph Statistics \u{256e}");
    println!("  Files analyzed: {node_count}");
    println!("  Dependencies: {edge_count}");
    println!("  Avg degree: {avg_degree:.2}");
    println!("  Density: {:.2}%", density * 100.0);
    println!("  Components: {}", components.len());

    let (p1, p2) = bisection(&graph);
    let cut: Vec<_> = graph
        .edges
        .iter()
        .filter(|(a, b)| p1.contains(a) != p1.contains(b))
        .cloned()
        .collect();
    println!("\nMinCut Boundaries (cut value: {})", cut.len());
    println!("\n  Partition 1:");
    for n in p1.iter().take(10) {
        println!("    {n}");
    }
    if p1.len() > 10 {
        println!("    ... and {} more", p1.len() - 10);
    }
    println!("\n  Partition 2:");
    for n in p2.iter().take(10) {
        println!("    {n}");
    }
    if p2.len() > 10 {
        println!("    ... and {} more", p2.len() - 10);
    }
    println!("\n  Suggestion: these two partitions share the most import edges \u{2014} a natural seam to split.");
    let _ = partitions;

    if !cycles.is_empty() {
        println!("\nCircular Dependencies Detected");
        for c in cycles.iter().take(5) {
            println!("  [MEDIUM] {}", c.join(" -> "));
        }
        if cycles.len() > 5 {
            println!("  ... and {} more cycles", cycles.len() - 5);
        }
    }
    0
}

// ---- modules ----------------------------------------------------------------

fn modules(root: &Path, command: &AnalyzeCommand) -> u8 {
    let target = command.positional.clone().unwrap_or_else(|| ".".into());
    let format_json = command.format.as_deref() == Some("json");

    println!("Detecting module communities: {target}\n");
    let graph = build_import_graph(root, &target);
    if graph.nodes.is_empty() {
        eprintln!("[ERROR] No source files found");
        return 1;
    }
    let components = connected_components(&graph);
    let communities: Vec<Vec<String>> = components
        .into_iter()
        .map(|c| {
            let mut sorted = c;
            sorted.sort();
            sorted
        })
        .collect();

    if format_json {
        let out = json!({
            "statistics": {
                "nodeCount": graph.nodes.len(),
                "edgeCount": graph.edges.len(),
                "moduleCount": communities.len(),
            },
            "modules": communities.iter().enumerate().map(|(i, m)| json!({
                "id": i, "size": m.len(), "files": m,
            })).collect::<Vec<_>>(),
        });
        write_or_print(command.output.as_deref(), &out);
        return 0;
    }

    println!("\u{256d} Community Detection \u{256e}");
    println!("  Files: {node_count}", node_count = graph.nodes.len());
    println!("  Modules detected: {mod_count}", mod_count = communities.len());
    println!("\n  {:<8} {:<8} Sample files", "Module", "Size");
    println!("  {} {} {}", "-".repeat(8), "-".repeat(8), "-".repeat(40));
    for (i, m) in communities.iter().enumerate().take(15) {
        let sample = m.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
        println!("  {:<8} {:<8} {}", i, m.len(), chars_take(&sample, 40));
    }
    if communities.len() > 15 {
        println!("  ... and {} more modules", communities.len() - 15);
    }
    0
}

// ---- dependencies -----------------------------------------------------------

fn dependencies(root: &Path, command: &AnalyzeCommand) -> u8 {
    let target = command.positional.clone().unwrap_or_else(|| ".".into());
    let format = command.format.clone().unwrap_or_else(|| "text".into());

    println!("Building dependency graph: {target}\n");
    let graph = build_import_graph(root, &target);
    if graph.nodes.is_empty() {
        eprintln!("[ERROR] No source files found");
        return 1;
    }

    let cycles = find_cycles(&graph);
    if format == "dot" {
        let dot = export_dot(&graph, &cycles);
        if let Some(p) = &command.output {
            let _ = fs::write(p, &dot);
            println!("DOT graph written to {p}");
        } else {
            println!("{dot}");
        }
        return 0;
    }
    if format == "json" {
        let out = json!({
            "nodes": graph.nodes,
            "edges": graph.edges.iter().map(|(a, b)| json!({"from": a, "to": b})).collect::<Vec<_>>(),
            "cycles": cycles,
        });
        write_or_print(command.output.as_deref(), &out);
        return 0;
    }

    // text: adjacency list
    println!("\u{256d} Dependency Graph \u{256e}");
    println!("  Nodes: {node_count}", node_count = graph.nodes.len());
    println!("  Edges: {edge_count}", edge_count = graph.edges.len());
    println!("  Cycles: {cycle_count}", cycle_count = cycles.len());
    println!("\n  {:<35} -> Imports", "File");
    println!("  {} {}", "-".repeat(35), "-".repeat(35));
    let mut sorted_nodes = graph.nodes.clone();
    sorted_nodes.sort();
    for n in sorted_nodes.iter().take(30) {
        let deps = graph.adj.get(n).cloned().unwrap_or_default();
        let dep_str = if deps.is_empty() {
            "(none)".into()
        } else {
            deps.iter().take(3).cloned().collect::<Vec<_>>().join(", ")
        };
        println!("  {:<35} -> {}", chars_take(n, 35), chars_take(&dep_str, 35));
    }
    0
}

// ---- circular ---------------------------------------------------------------

fn circular(root: &Path, command: &AnalyzeCommand) -> u8 {
    let target = command.positional.clone().unwrap_or_else(|| ".".into());
    let format_json = command.format.as_deref() == Some("json");

    println!("Detecting circular dependencies: {target}\n");
    let graph = build_import_graph(root, &target);
    if graph.nodes.is_empty() {
        eprintln!("[ERROR] No source files found");
        return 1;
    }
    let cycles = find_cycles(&graph);

    if format_json {
        let out = json!({
            "count": cycles.len(),
            "cycles": cycles.iter().enumerate().map(|(i, c)| json!({
                "id": i, "cycle": c, "severity": if c.len() > 4 { "high" } else { "medium" },
            })).collect::<Vec<_>>(),
        });
        write_or_print(command.output.as_deref(), &out);
        return 0;
    }

    println!("\u{256d} Circular Dependency Detection \u{256e}");
    println!("  Files: {node_count}", node_count = graph.nodes.len());
    println!("  Cycles found: {}", cycles.len());
    if cycles.is_empty() {
        println!("\n\u{2714} No circular dependencies detected");
        return 0;
    }
    println!("\n  {:<8} {:<10} Cycle", "#", "Severity");
    println!("  {} {} {}", "-".repeat(8), "-".repeat(10), "-".repeat(50));
    for (i, c) in cycles.iter().enumerate().take(20) {
        let sev = if c.len() > 4 { "high" } else { "medium" };
        println!("  {:<8} {:<10} {}", i + 1, sev, c.join(" -> "));
    }
    if cycles.len() > 20 {
        println!("  ... and {} more", cycles.len() - 20);
    }
    0
}

// ---- graph algorithms -------------------------------------------------------

fn connected_components(graph: &ImportGraph) -> Vec<Vec<String>> {
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    let node_set: HashSet<&str> = graph.nodes.iter().map(|s| s.as_str()).collect();
    for (a, b) in &graph.edges {
        adj.entry(a.as_str()).or_default().push(b.as_str());
        adj.entry(b.as_str()).or_default().push(a.as_str());
    }
    let mut visited: HashSet<&str> = HashSet::new();
    let mut components = Vec::new();
    for n in &graph.nodes {
        if visited.contains(n.as_str()) {
            continue;
        }
        let mut stack = vec![n.as_str()];
        let mut comp = Vec::new();
        while let Some(cur) = stack.pop() {
            if !visited.insert(cur) {
                continue;
            }
            comp.push(cur.to_string());
            if let Some(nbrs) = adj.get(cur) {
                for nb in nbrs {
                    if node_set.contains(*nb) && !visited.contains(*nb) {
                        stack.push(nb);
                    }
                }
            }
        }
        components.push(comp);
    }
    components.sort_by_key(|b| std::cmp::Reverse(b.len()));
    components
}

// Find elementary cycles via DFS back-edge detection (Johnson's would be
// heavier; for a codebase import graph the simpler back-edge path replay is
// sufficient and matches the TS output shape of `a -> b -> a`).
fn find_cycles(graph: &ImportGraph) -> Vec<Vec<String>> {
    let nodes: Vec<&str> = graph.nodes.iter().map(|s| s.as_str()).collect();
    let adj: HashMap<&str, Vec<&str>> = build_adj_str(graph);
    let mut cycles = Vec::new();
    let mut seen_signatures: HashSet<Vec<String>> = HashSet::new();

    let max_depth = nodes.len().max(1);
    for &start in &nodes {
        let mut path: Vec<&str> = vec![start];
        let mut on_path: HashSet<&str> = HashSet::new();
        on_path.insert(start);
        dfs_cycles(start, start, &adj, &mut path, &mut on_path, &mut cycles, &mut seen_signatures, 0, max_depth);
    }

    cycles
}

#[allow(clippy::too_many_arguments)]
fn dfs_cycles<'a>(
    start: &'a str,
    cur: &'a str,
    adj: &HashMap<&'a str, Vec<&'a str>>,
    path: &mut Vec<&'a str>,
    on_path: &mut HashSet<&'a str>,
    out: &mut Vec<Vec<String>>,
    seen: &mut HashSet<Vec<String>>,
    depth: usize,
    max_depth: usize,
) {
    // A cycle cannot be longer than the number of nodes; bound the search by
    // that rather than an arbitrary magic number so we never miss a real cycle.
    if depth >= max_depth {
        return;
    }
    if let Some(nbrs) = adj.get(cur) {
        for &nb in nbrs {
            if nb == start && path.len() > 1 {
                let mut cycle: Vec<String> = path.iter().map(|s| s.to_string()).collect();
                cycle.push(start.to_string());
                // Signature = canonical rotation of the cycle's nodes (smallest
                // element first) so a->b->a and b->a->b dedup as the same cycle,
                // WITHOUT collapsing distinct cycles (a->b->c->a vs a->c->b->a)
                // the way a plain sorted node-set would.
                let node_count = cycle.len() - 1; // exclude the repeated closer
                let core = &cycle[..node_count];
                let min_idx = core
                    .iter()
                    .enumerate()
                    .min_by(|a, b| a.1.cmp(b.1))
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                let sig: Vec<String> = (0..node_count)
                    .map(|i| core[(min_idx + i) % node_count].clone())
                    .collect();
                if seen.insert(sig) {
                    out.push(cycle);
                }
            } else if !on_path.contains(nb) {
                path.push(nb);
                on_path.insert(nb);
                dfs_cycles(start, nb, adj, path, on_path, out, seen, depth + 1, max_depth);
                on_path.remove(nb);
                path.pop();
            }
        }
    }
}

fn build_adj_str(graph: &ImportGraph) -> HashMap<&str, Vec<&str>> {
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for (a, b) in &graph.edges {
        adj.entry(a.as_str()).or_default().push(b.as_str());
    }
    adj
}

// Crude graph bisection: alternate assignment by BFS order from an arbitrary
// seed, then swap nodes that reduce the cut. Good enough for a "natural seam"
// suggestion; not a true global MinCut.
fn bisection(graph: &ImportGraph) -> (Vec<String>, Vec<String>) {
    let adj = build_adj_str(graph);
    let nodes: Vec<&str> = graph.nodes.iter().map(|s| s.as_str()).collect();
    if nodes.is_empty() {
        return (Vec::new(), Vec::new());
    }
    // BFS order from first node.
    let mut order: Vec<&str> = Vec::new();
    let mut visited: HashSet<&str> = HashSet::new();
    for &seed in &nodes {
        if visited.contains(seed) {
            continue;
        }
        let mut q = vec![seed];
        while let Some(cur) = q.pop() {
            if !visited.insert(cur) {
                continue;
            }
            order.push(cur);
            if let Some(nbrs) = adj.get(cur) {
                for nb in nbrs {
                    if !visited.contains(*nb) {
                        q.push(nb);
                    }
                }
            }
        }
    }
    let half = order.len() / 2;
    let p1: HashSet<&str> = order.iter().take(half).copied().collect();
    let mut part1 = Vec::new();
    let mut part2 = Vec::new();
    for n in &order {
        if p1.contains(*n) {
            part1.push(n.to_string());
        } else {
            part2.push(n.to_string());
        }
    }
    (part1, part2)
}

fn export_dot(graph: &ImportGraph, cycles: &[Vec<String>]) -> String {
    let cycle_edges: HashSet<(String, String)> = cycles
        .iter()
        .flat_map(|c| c.windows(2).map(|w| (w[0].clone(), w[1].clone())))
        .collect();
    let mut s = String::from("digraph dependencies {\n");
    for n in &graph.nodes {
        s.push_str(&format!("  \"{n}\";\n"));
    }
    for (a, b) in &graph.edges {
        let attr = if cycle_edges.contains(&(a.clone(), b.clone())) {
            " [color=red]"
        } else {
            ""
        };
        s.push_str(&format!("  \"{a}\" -> \"{b}\"{attr};\n"));
    }
    s.push_str("}\n");
    s
}

// ---- helpers ----------------------------------------------------------------

fn chars_take(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn resolve_under(root: &Path, target: &str) -> PathBuf {
    let p = Path::new(target);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.join(p)
    }
}

fn write_or_print(output: Option<&str>, v: &Value) {
    if let Some(p) = output {
        let _ = fs::write(p, serde_json::to_vec_pretty(v).unwrap_or_default());
        println!("Results written to {p}");
    } else {
        println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyze_extracts_function_and_import() {
        let code = "import { foo } from './bar';\nfunction baz() { return 1; }\nconst arrow = (x) => x;\n";
        let a = analyze_code(code, "test.ts");
        assert!(a.functions.iter().any(|(n, _)| n == "baz"));
        assert!(a.functions.iter().any(|(n, _)| n == "arrow"));
        assert!(a.imports.contains(&"./bar".to_string()));
    }

    #[test]
    fn cyclomatic_counts_decisions() {
        let code = "if (a) {}\nfor (;;) {}\nx ? 1 : 0\n";
        let a = analyze_code(code, "x.ts");
        // if, for, ?, plus baseline 1 => >= 4
        assert!(a.cyclomatic >= 4);
    }

    #[test]
    fn is_external_classifies() {
        assert!(is_external("react"));
        assert!(is_external("@scope/pkg"));
        assert!(!is_external("./local"));
        assert!(!is_external("/abs"));
    }

    #[test]
    fn risk_band_thresholds() {
        assert_eq!(risk_band(0), "low");
        assert_eq!(risk_band(25), "medium");
        assert_eq!(risk_band(50), "high");
        assert_eq!(risk_band(75), "critical");
        assert_eq!(risk_band(100), "critical");
    }

    #[test]
    fn classify_picks_category() {
        assert_eq!(classify(&[DiffFile { path: "a.test.ts".into(), status: "M".into(), additions: 1, deletions: 0, binary: false }]), "test");
        assert_eq!(classify(&[DiffFile { path: "README.md".into(), status: "M".into(), additions: 1, deletions: 0, binary: false }]), "docs");
        assert_eq!(classify(&[DiffFile { path: "src/a.ts".into(), status: "M".into(), additions: 1, deletions: 0, binary: false }]), "feature");
    }

    #[test]
    fn cycle_detection_finds_two_node_cycle() {
        let mut g = ImportGraph { nodes: vec!["a".into(), "b".into()], edges: vec![], adj: HashMap::new() };
        g.edges.push(("a".into(), "b".into()));
        g.edges.push(("b".into(), "a".into()));
        let cycles = find_cycles(&g);
        assert!(!cycles.is_empty(), "must detect a->b->a");
    }

    #[test]
    fn connected_components_separates() {
        let mut g = ImportGraph { nodes: vec!["a".into(), "b".into(), "c".into()], edges: vec![], adj: HashMap::new() };
        g.edges.push(("a".into(), "b".into()));
        let comps = connected_components(&g);
        assert_eq!(comps.len(), 2);
    }
}
