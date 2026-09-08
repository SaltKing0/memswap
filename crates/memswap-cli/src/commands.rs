use std::path::PathBuf;

use clap::Args;
use memswap_adapters::AdapterRegistry;
use memswap_core::{Error, Store};

pub const EXIT_OK: i32 = 0;
pub const EXIT_ERROR: i32 = 1;
// EXIT_USAGE / EXIT_CONFLICT are part of the documented exit-code contract but
// unused in M1 (migrate/diff are M2); kept for the stable ABI.
#[allow(dead_code)]
pub const EXIT_USAGE: i32 = 2;
pub const EXIT_NOT_FOUND: i32 = 3;
pub const EXIT_VERIFY: i32 = 4;
#[allow(dead_code)]
pub const EXIT_CONFLICT: i32 = 5;

#[derive(Args)]
pub struct InitArgs {
    /// Directory to create the store in (default ./memory.memfile).
    #[arg(long)]
    pub dir: Option<PathBuf>,
    #[arg(long)]
    pub package: Option<String>,
    #[arg(long)]
    pub harness: Option<String>,
}

#[derive(Args)]
pub struct ExportArgs {
    /// Harness to export: AUTO | hermes | codex | claude.
    #[arg(long, default_value = "AUTO")]
    pub harness: String,
    /// Harness home dir (default: ~/.hermes, ~/.codex, ~/.claude).
    #[arg(long)]
    pub home: Option<PathBuf>,
    /// Output store dir (default ./memory.memfile).
    #[arg(long)]
    pub out: Option<PathBuf>,
}

#[derive(Args)]
pub struct ImportArgs {
    #[arg(long)]
    pub harness: String,
    #[arg(long)]
    pub home: Option<PathBuf>,
    /// Store dir to import from (default ./memory.memfile).
    #[arg(long)]
    pub dir: Option<PathBuf>,
    /// Print the plan and write nothing.
    #[arg(long)]
    pub dry_run: bool,
    /// Merge strategy: replace | merge | keep (default merge).
    #[arg(long, default_value = "merge")]
    pub merge: String,
}

#[derive(Args)]
pub struct VerifyArgs {
    /// Store dir (default ./memory.memfile).
    #[arg(long)]
    pub dir: Option<PathBuf>,
    #[arg(long)]
    pub strict: bool,
}

#[derive(Args)]
pub struct DoctorArgs {
    /// Base home to probe (default $HOME).
    #[arg(long)]
    pub home: Option<PathBuf>,
}

#[derive(Args)]
pub struct AdaptersArgs {
    #[command(subcommand)]
    pub sub: AdaptersSub,
}

#[derive(clap::Subcommand)]
pub enum AdaptersSub {
    /// List built-in and loaded adapters.
    List,
    /// Show details for one adapter.
    Show { name: String },
}

#[derive(Args)]
pub struct LogArgs {
    #[arg(long)]
    pub dir: Option<PathBuf>,
    /// Show full entry snapshots per commit (default: summary only).
    #[arg(long)]
    pub full: bool,
}

#[derive(Args)]
pub struct DiffArgs {
    /// From-revision: HEAD | HEAD~N | <hash> (default HEAD~1).
    #[arg(long)]
    pub from: Option<String>,
    /// To-revision (default HEAD).
    #[arg(long)]
    pub to: Option<String>,
    #[arg(long)]
    pub dir: Option<PathBuf>,
}

#[derive(Args)]
pub struct KeygenArgs {
    /// Write the hex secret key to this file (created with 0600 perms).
    #[arg(long)]
    pub out: PathBuf,
}

#[derive(Args)]
pub struct SignArgs {
    /// Store dir (default ./memory.memfile).
    #[arg(long)]
    pub dir: Option<PathBuf>,
    /// File holding the hex ed25519 secret key.
    #[arg(long)]
    pub key: PathBuf,
}

#[derive(Args)]
pub struct MigrateArgs {
    #[arg(long)]
    pub from: Option<u32>,
    #[arg(long)]
    pub to: Option<u32>,
    #[arg(long)]
    pub dir: Option<PathBuf>,
}

pub fn run(cmd: crate::Command, json: bool) -> i32 {
    match cmd {
        crate::Command::Init(a) => cmd_init(a, json),
        crate::Command::Export(a) => cmd_export(a, json),
        crate::Command::Import(a) => cmd_import(a, json),
        crate::Command::Verify(a) => cmd_verify(a, json),
        crate::Command::Doctor(a) => cmd_doctor(a, json),
        crate::Command::Adapters(a) => cmd_adapters(a, json),
        crate::Command::Log(a) => cmd_log(a, json),
        crate::Command::Diff(a) => cmd_diff(a, json),
        crate::Command::Keygen(a) => cmd_keygen(a, json),
        crate::Command::Sign(a) => cmd_sign(a, json),
        crate::Command::Migrate(_) => not_impl("migrate", "M6"),
    }
}

fn not_impl(cmd: &str, milestone: &str) -> i32 {
    eprintln!("`mem {cmd}` is not implemented in M1 (planned for {milestone}).");
    EXIT_ERROR
}

fn default_home(harness: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let base = PathBuf::from(home);
    match harness.to_lowercase().as_str() {
        "hermes" => base.join(".hermes"),
        "codex" => base.join(".codex"),
        "claude" => base.join(".claude"),
        _ => base,
    }
}

fn store_dir(explicit: Option<PathBuf>) -> PathBuf {
    explicit.unwrap_or_else(|| PathBuf::from("memory.memfile"))
}

fn print_obj(json: bool, value: &serde_json::Value) {
    if json {
        println!("{}", serde_json::to_string(value).unwrap());
    } else {
        println!("{}", serde_json::to_string_pretty(value).unwrap());
    }
}

fn cmd_init(a: InitArgs, json: bool) -> i32 {
    let dir = store_dir(a.dir);
    let package = a.package.unwrap_or_else(|| "memory".into());
    let harness = a.harness.unwrap_or_else(|| "unknown".into());
    match Store::init(&dir, &package, &harness, None) {
        Ok(_) => {
            print_obj(
                json,
                &serde_json::json!({ "ok": true, "store": dir.display().to_string() }),
            );
            EXIT_OK
        }
        Err(e) => {
            eprintln!("error: {e}");
            EXIT_ERROR
        }
    }
}

fn cmd_export(a: ExportArgs, json: bool) -> i32 {
    let registry = AdapterRegistry::builtin();
    let harness = if a.harness.eq_ignore_ascii_case("AUTO") {
        // Pick the first detected harness under the default homes.
        let home = default_home("hermes");
        let detected = registry.detect_all(&home);
        match detected.iter().find(|(_, c)| c.is_some()) {
            Some((name, _)) => name.clone(),
            None => {
                eprintln!("error: no harness detected under {}", home.display());
                return EXIT_NOT_FOUND;
            }
        }
    } else {
        a.harness.to_lowercase()
    };
    let home = a.home.unwrap_or_else(|| default_home(&harness));
    let out = store_dir(a.out);

    let entries = match registry.read_harness(&harness, &home) {
        Ok(e) => e,
        Err(Error::Adapter(msg)) => {
            eprintln!("error: {msg}");
            return EXIT_NOT_FOUND;
        }
        Err(e) => {
            eprintln!("error: {e}");
            return EXIT_ERROR;
        }
    };

    // Upsert: init a fresh store or update an existing one (history chains).
    let store = match Store::open(&out) {
        Ok(s) => s,
        Err(_) => match Store::init(&out, "memory", &harness, None) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: {e}");
                return EXIT_ERROR;
            }
        },
    };
    if let Err(e) = store.replace_entries(&entries) {
        eprintln!("error: {e}");
        return EXIT_ERROR;
    }
    print_obj(
        json,
        &serde_json::json!({
            "ok": true,
            "harness": harness,
            "entries": entries.len(),
            "store": out.display().to_string(),
        }),
    );
    EXIT_OK
}

fn cmd_import(a: ImportArgs, json: bool) -> i32 {
    let registry = AdapterRegistry::builtin();
    let harness = a.harness.to_lowercase();
    let home = a.home.unwrap_or_else(|| default_home(&harness));
    let dir = store_dir(a.dir);

    let store = match Store::open(&dir) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return EXIT_ERROR;
        }
    };
    let entries = match store.read_entries() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("error: {e}");
            return EXIT_ERROR;
        }
    };

    let adapter = match registry.get(&harness) {
        Some(a) => a,
        None => {
            eprintln!("error: no adapter '{}'", harness);
            return EXIT_NOT_FOUND;
        }
    };
    let ctx = match adapter.detect(&home) {
        Some(c) => c,
        None => {
            eprintln!(
                "error: harness '{}' not detected under {}",
                harness,
                home.display()
            );
            return EXIT_NOT_FOUND;
        }
    };

    let strategy = match a.merge.as_str() {
        "replace" => memswap_adapters::MergeStrategy::Replace,
        "keep" => memswap_adapters::MergeStrategy::Keep,
        _ => memswap_adapters::MergeStrategy::Merge,
    };

    if a.dry_run {
        print_obj(
            json,
            &serde_json::json!({
                "dry_run": true,
                "harness": harness,
                "would_write": entries.len(),
                "strategy": a.merge,
            }),
        );
        return EXIT_OK;
    }

    match adapter.write(&ctx, &entries, strategy) {
        Ok(report) => {
            print_obj(
                json,
                &serde_json::json!({
                    "ok": true,
                    "harness": harness,
                    "written": report.written,
                    "truncated": report.truncated,
                    "skipped": report.skipped,
                }),
            );
            EXIT_OK
        }
        Err(Error::Adapter(msg)) => {
            eprintln!("error: {msg}");
            EXIT_NOT_FOUND
        }
        Err(e) => {
            eprintln!("error: {e}");
            EXIT_ERROR
        }
    }
}

fn cmd_verify(a: VerifyArgs, json: bool) -> i32 {
    let dir = store_dir(a.dir);
    let store = match Store::open(&dir) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return EXIT_ERROR;
        }
    };
    match store.verify() {
        Ok(report) => {
            print_obj(
                json,
                &serde_json::json!({
                    "ok": report.ok,
                    "entries": report.entries,
                    "objects_ok": report.objects_ok,
                    "refs_ok": report.refs_ok,
                    "manifest_ok": report.manifest_ok,
                    "chain_ok": report.chain_ok,
                    "commits": report.commits,
                    "signature_ok": report.signature_ok,
                }),
            );
            if report.ok {
                EXIT_OK
            } else {
                EXIT_VERIFY
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            EXIT_ERROR
        }
    }
}

fn cmd_log(a: LogArgs, json: bool) -> i32 {
    let dir = store_dir(a.dir);
    match memswap_core::history::log(&dir) {
        Ok(chain) => {
            let commits: Vec<_> = chain
                .iter()
                .map(|c| {
                    let mut row = serde_json::json!({
                        "commit": c.commit_hash,
                        "parent": if c.parent_hash.is_empty() { serde_json::Value::Null } else { serde_json::json!(c.parent_hash) },
                        "tree": c.tree_hash,
                        "timestamp": c.timestamp,
                        "message": c.message,
                        "entries": c.entries.len(),
                    });
                    if a.full {
                        row["entry_ids"] = serde_json::json!(
                            c.entries.iter().map(|e| e.id.clone()).collect::<Vec<_>>()
                        );
                    }
                    row
                })
                .collect();
            print_obj(json, &serde_json::json!({ "commits": commits }));
            EXIT_OK
        }
        Err(e) => {
            eprintln!("error: {e}");
            EXIT_ERROR
        }
    }
}

fn cmd_diff(a: DiffArgs, json: bool) -> i32 {
    let dir = store_dir(a.dir);
    match memswap_core::history::diff(&dir, a.from.as_deref(), a.to.as_deref()) {
        Ok(report) => {
            print_obj(
                json,
                &serde_json::json!({
                    "from": report.from,
                    "to": report.to,
                    "added": report.added,
                    "removed": report.removed,
                    "changed": report.changed,
                    "empty": report.is_empty(),
                }),
            );
            EXIT_OK
        }
        Err(memswap_core::Error::NotFound(msg)) => {
            eprintln!("error: {msg}");
            EXIT_NOT_FOUND
        }
        Err(e) => {
            eprintln!("error: {e}");
            EXIT_ERROR
        }
    }
}

fn cmd_keygen(a: KeygenArgs, json: bool) -> i32 {
    match memswap_core::sign::keygen() {
        Ok(kp) => {
            if let Err(e) = write_secret_file(&a.out, &kp.secret_hex) {
                eprintln!("error: {e}");
                return EXIT_ERROR;
            }
            print_obj(
                json,
                &serde_json::json!({
                    "ok": true,
                    "key_file": a.out.display().to_string(),
                    "public_key": kp.public_hex,
                }),
            );
            EXIT_OK
        }
        Err(e) => {
            eprintln!("error: {e}");
            EXIT_ERROR
        }
    }
}

fn cmd_sign(a: SignArgs, json: bool) -> i32 {
    let dir = store_dir(a.dir);
    let secret = match memswap_core::sign::load_secret(&a.key) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return EXIT_ERROR;
        }
    };
    match memswap_core::sign::sign_store(&dir, &secret) {
        Ok(doc) => {
            print_obj(
                json,
                &serde_json::json!({
                    "ok": true,
                    "store": dir.display().to_string(),
                    "public_key": doc.public_key,
                    "message": doc.message,
                }),
            );
            EXIT_OK
        }
        Err(e) => {
            eprintln!("error: {e}");
            EXIT_ERROR
        }
    }
}

/// Write a hex secret key with owner-only permissions.
fn write_secret_file(path: &PathBuf, secret: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(secret.as_bytes())?;
    Ok(())
}

fn cmd_doctor(a: DoctorArgs, json: bool) -> i32 {
    let registry = AdapterRegistry::builtin();
    let base = a
        .home
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or(".".into())));
    // Probe each harness's own home under the base ($HOME/.hermes, $HOME/.codex, ...).
    let rows: Vec<_> = registry
        .names()
        .iter()
        .map(|name| {
            let home = base.join(format!(".{name}"));
            let ctx = registry.get(name).and_then(|a| a.detect(&home));
            serde_json::json!({
                "harness": name,
                "detected": ctx.is_some(),
                "home": home.display().to_string(),
                "files": ctx.map(|c| c.detected.iter().map(|(l, p)| serde_json::json!({"label": l, "path": p.display().to_string()})).collect::<Vec<_>>()).unwrap_or_default(),
            })
        })
        .collect();
    print_obj(
        json,
        &serde_json::json!({ "base": base.display().to_string(), "harnesses": rows }),
    );
    EXIT_OK
}

fn cmd_adapters(a: AdaptersArgs, json: bool) -> i32 {
    let registry = AdapterRegistry::builtin();
    match a.sub {
        AdaptersSub::List => {
            let names = registry.names();
            print_obj(json, &serde_json::json!({ "adapters": names }));
        }
        AdaptersSub::Show { name } => match registry.get(&name) {
            Some(_) => print_obj(
                json,
                &serde_json::json!({ "adapter": name.to_lowercase(), "available": true }),
            ),
            None => {
                eprintln!("error: no adapter '{}'", name);
                return EXIT_NOT_FOUND;
            }
        },
    }
    EXIT_OK
}
