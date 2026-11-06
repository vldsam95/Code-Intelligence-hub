use anyhow::{anyhow, Context, Result};
use clap::{Args, Parser, Subcommand};
use codeintel_core::{
    compute_source_revision, data_root, discover_project, discover_wiki, fork_project,
    initialize_project, load_project, project_paths, write_portable_project_files, ProjectManifest,
    WikiConfig, PRODUCT_VERSION,
};
use codeintel_mcp::serve_stdio;
use codeintel_worker::{
    clear_project_hub_secrets, config_path, configure_project_hub_binding, handle_request,
    load_config, load_registry, open_project, rebuild_index, run_daemon, save_config,
    send_local_request, RpcRequest, WorkerConfig, DEFAULT_SOCKET,
};
use reqwest::blocking::Client;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

#[derive(Parser)]
#[command(
    name = "codeintel",
    version,
    about = "Code Intelligence Worker, CLI and MCP"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Init(InitArgs),
    Open(PathArgs),
    Index(PathArgs),
    Revision(PathArgs),
    Status(PathArgs),
    Preflight(PreflightArgs),
    Review(RunArgs),
    Verify(RunArgs),
    Gate(RunArgs),
    Security(SecurityArgs),
    Abandon(AbandonArgs),
    ForkProject(PathArgs),
    RecoverProject(RecoverArgs),
    DeadCode(PathArgs),
    Doctor(DoctorArgs),
    Enroll(EnrollArgs),
    #[command(subcommand)]
    Hub(HubCommand),
    #[command(subcommand)]
    Worker(WorkerCommand),
    #[command(subcommand)]
    Mcp(McpCommand),
}

#[derive(Args)]
struct InitArgs {
    #[arg(default_value = ".")]
    path: PathBuf,
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    slug: Option<String>,
    #[arg(long, value_parser = ["none", "local", "remote"])]
    wiki_mode: Option<String>,
    #[arg(long)]
    wiki_id: Option<String>,
    #[arg(long)]
    wiki_path: Option<PathBuf>,
}

#[derive(Args)]
struct PathArgs {
    #[arg(default_value = ".")]
    path: PathBuf,
}

#[derive(Args)]
struct PreflightArgs {
    #[arg(default_value = ".")]
    path: PathBuf,
    #[arg(long)]
    intent: String,
    #[arg(long = "file")]
    planned_files: Vec<String>,
    #[arg(long)]
    agent_client: Option<String>,
}

#[derive(Args)]
struct RunArgs {
    #[arg(default_value = ".")]
    path: PathBuf,
    #[arg(long)]
    change_run_id: Uuid,
}

#[derive(Args)]
struct SecurityArgs {
    #[arg(default_value = ".")]
    path: PathBuf,
    #[arg(long)]
    change_run_id: Uuid,
    #[arg(long, value_parser = ["inventory", "validate", "run", "correlate"], default_value = "validate")]
    action: String,
    #[arg(long = "scenario")]
    scenario_ids: Vec<String>,
    #[arg(long, default_value_t = 1)]
    page: usize,
    #[arg(long, default_value_t = 25)]
    limit: usize,
}

#[derive(Args)]
struct AbandonArgs {
    #[arg(default_value = ".")]
    path: PathBuf,
    #[arg(long)]
    change_run_id: Uuid,
    #[arg(long)]
    reason: String,
}

#[derive(Args)]
struct RecoverArgs {
    #[arg(default_value = ".")]
    path: PathBuf,
    #[arg(long)]
    hub: String,
    #[arg(long, env = "CODEINTEL_RECOVERY_TOKEN", hide_env_values = true)]
    token: String,
}

#[derive(Args)]
struct DoctorArgs {
    #[arg(long)]
    client: Option<String>,
}

#[derive(Args)]
struct EnrollArgs {
    #[arg(long)]
    hub: String,
    #[arg(long, env = "CODEINTEL_ENROLL_TOKEN", hide_env_values = true)]
    token: String,
    #[arg(long, value_delimiter = ',', default_value = "codex,claude,hermes")]
    clients: Vec<String>,
}

#[derive(Subcommand)]
enum HubCommand {
    Bind(HubBindArgs),
}

#[derive(Args)]
struct HubBindArgs {
    #[arg(default_value = ".")]
    path: PathBuf,
    #[arg(long)]
    hub: String,
    #[arg(long, env = "CODEINTEL_PROJECT_TOKEN", hide_env_values = true)]
    project_token: String,
}

#[derive(Subcommand)]
enum WorkerCommand {
    Run,
}

#[derive(Subcommand)]
enum McpCommand {
    Serve {
        #[arg(long, default_value_t = true)]
        stdio: bool,
        #[arg(long)]
        socket: Option<PathBuf>,
    },
}

fn main() {
    if let Err(error) = run() {
        eprintln!(
            "{}",
            json!({
                "ok": false,
                "error": {
                    "code": classify_error(&error.to_string()),
                    "message": error.to_string()
                }
            })
        );
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init(args) => init(args),
        Command::Open(args) => invoke(
            "open_project",
            json!({"path": canonical_request_path(&args.path)?}),
        ),
        Command::Index(args) => index(args.path),
        Command::Revision(args) => revision(args.path),
        Command::Status(args) => invoke(
            "project_status",
            json!({"path": canonical_request_path(&args.path)?}),
        ),
        Command::Preflight(args) => invoke(
            "preflight_change",
            json!({
                "path": canonical_request_path(&args.path)?,
                "intent": args.intent,
                "planned_files": args.planned_files,
                "agent_client": args.agent_client,
            }),
        ),
        Command::Review(args) => invoke(
            "review_change",
            json!({"path": canonical_request_path(&args.path)?, "change_run_id": args.change_run_id}),
        ),
        Command::Verify(args) => invoke(
            "verify_change",
            json!({"path": canonical_request_path(&args.path)?, "change_run_id": args.change_run_id}),
        ),
        Command::Gate(args) => invoke(
            "quality_gate",
            json!({"path": canonical_request_path(&args.path)?, "change_run_id": args.change_run_id}),
        ),
        Command::Security(args) => invoke(
            "security_scenarios",
            json!({
                "path": canonical_request_path(&args.path)?,
                "change_run_id": args.change_run_id,
                "action": args.action,
                "scenario_ids": args.scenario_ids,
                "page": args.page,
                "limit": args.limit,
            }),
        ),
        Command::Abandon(args) => invoke(
            "abandon_change_run",
            json!({
                "path": canonical_request_path(&args.path)?,
                "change_run_id": args.change_run_id,
                "reason": args.reason,
            }),
        ),
        Command::ForkProject(args) => {
            let root = discover_project(&args.path)?;
            let (parent, project_id) = fork_project(&root)?;
            clear_project_hub_secrets(&root)?;
            let configured_hub = load_config().ok().and_then(|value| value.hub_url);
            configure_project_hub_binding(
                &root,
                configured_hub.as_deref(),
                "AUTO_CREATE",
                true,
                None,
            )?;
            print_json(json!({
                "parent_project_id": parent,
                "project_id": project_id,
                "next": "Run codeintel open to register the new logical project."
            }))
        }
        Command::RecoverProject(args) => recover_project(args),
        Command::DeadCode(args) => invoke("advanced.dead_code_audit", json!({"path": args.path})),
        Command::Doctor(args) => doctor(args),
        Command::Enroll(args) => enroll(args),
        Command::Hub(HubCommand::Bind(args)) => bind_project_hub(args),
        Command::Worker(WorkerCommand::Run) => run_daemon(load_config()?),
        Command::Mcp(McpCommand::Serve { stdio, socket }) => {
            if !stdio {
                return Err(anyhow!("Only stdio MCP is supported in V1"));
            }
            serve_stdio(socket)
        }
    }
}

fn init(args: InitArgs) -> Result<()> {
    let root =
        fs::canonicalize(&args.path).with_context(|| format!("resolve {}", args.path.display()))?;
    let inferred_name = root
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let name = args.name.clone().unwrap_or_else(|| inferred_name.clone());
    let slug = args.slug.clone().unwrap_or_else(|| slugify(&inferred_name));
    let manifest_path = root.join(".code-intel/project.yaml");
    if !manifest_path.is_file()
        && load_registry()
            .ok()
            .is_some_and(|registry| registry.projects.values().any(|entry| entry.root == root))
    {
        return Err(anyhow!(
            "RECOVERY_REQUIRED: this path was previously registered but its manifest is missing; use codeintel recover-project"
        ));
    }
    let wiki = selected_wiki(&root, &args)?;
    let initialization = initialize_project(&root, &name, &slug, wiki)?;
    if initialization.created {
        let configured_hub = load_config().ok().and_then(|value| value.hub_url);
        configure_project_hub_binding(&root, configured_hub.as_deref(), "AUTO_CREATE", true, None)?;
    }
    let opened = open_for_init(&root)?;
    let sync = request_value("sync_hub", json!({}))?;
    print_json(json!({
        "status": if initialization.created { "INITIALIZED" } else { "ALREADY_INITIALIZED" },
        "project_id": opened["project_id"],
        "instance_id": opened["instance_id"],
        "source_revision_id": opened["current_source_revision_id"],
        "wiki": {
            "mode": opened["wiki"]["mode"],
            "binding_status": opened["wiki"]["binding_status"],
            "wiki_id": initialization.manifest.wiki.wiki_id,
            "local_path": initialization.manifest.wiki.local_path,
        },
        "hub_registration": sync["registration"],
        "hub_sync": sync["sync"],
        "automatic_retry": sync["automatic_retry"],
        "hub_message": sync.get("reason"),
        "workspace_mode": opened["workspace_mode"],
        "index": {
            "state": opened["index_state"],
            "readiness": opened["readiness"],
            "coverage": opened["coverage"],
        },
        "created": [
            ".code-intel/project.yaml",
            ".code-intel/policy.yaml",
            ".code-intel/AGENT_POLICY.md",
            ".code-intel/SKILL.md",
            ".code-intel/README_AGENT.md",
            ".code-intel/context/",
            ".code-intel/portability/",
            ".code-intel/hub.example.yaml"
        ],
        "doctor": "Run codeintel doctor after enrollment/client adapter installation.",
        "portable_identity": opened["project_id"],
        "local_success": true,
    }))?;
    Ok(())
}

fn index(path: PathBuf) -> Result<()> {
    let (manifest, policy, entry, revision) = open_project(&path)?;
    let result = rebuild_index(&entry.root, &manifest, &policy, &entry, &revision)?;
    print_json(serde_json::to_value(result)?)
}

fn revision(path: PathBuf) -> Result<()> {
    let root = discover_project(&path)?;
    let (_, policy) = load_project(&root)?;
    let revision = compute_source_revision(&root, &policy)?;
    print_json(json!({
        "source_revision_id": revision.source_revision_id,
        "inventory_hash": revision.inventory_hash,
        "policy_hash": revision.policy_hash,
        "file_count": revision.files.len(),
        "volatile_files": revision.volatile_files,
        "git_required": false,
    }))
}

fn invoke(method: &str, params: Value) -> Result<()> {
    print_json(request_value(method, params)?)
}

fn canonical_request_path(path: &std::path::Path) -> Result<PathBuf> {
    resolve_request_path(path, &std::env::current_dir()?)
}

fn resolve_request_path(path: &std::path::Path, current_dir: &std::path::Path) -> Result<PathBuf> {
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        current_dir.join(path)
    };
    fs::canonicalize(&candidate).with_context(|| format!("resolve {}", candidate.display()))
}

fn request_value(method: &str, params: Value) -> Result<Value> {
    let socket = std::env::var_os("CODEINTEL_WORKER_SOCKET")
        .map(PathBuf::from)
        .or_else(|| load_config().ok().map(|config| config.socket_path))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_SOCKET));
    let request = RpcRequest {
        request_id: Uuid::new_v4(),
        method: method.into(),
        params,
    };
    if socket.exists() {
        send_local_request(&socket, &request)
    } else {
        handle_request(&request)
    }
}

fn open_for_init(root: &std::path::Path) -> Result<Value> {
    let mut last_error = None;
    for _ in 0..25 {
        match request_value("open_project", json!({"path": root})) {
            Ok(value) => return Ok(value),
            Err(error) if error.to_string().contains("IN_USE") => {
                last_error = Some(error);
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow!("IN_USE: initialization did not converge")))
}

fn doctor(args: DoctorArgs) -> Result<()> {
    let config = load_config().ok();
    let socket = config
        .as_ref()
        .map(|value| value.socket_path.clone())
        .unwrap_or_else(|| PathBuf::from(DEFAULT_SOCKET));
    let mut checks = vec![];
    checks.push(json!({
        "id": "binary",
        "status": "PASS",
        "version": PRODUCT_VERSION,
        "rust_core": true,
    }));
    if let Some(worker) = config.as_ref() {
        if let Some(hub) = worker.hub_url.as_ref() {
            let reachable = Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(8))
                .build()
                .and_then(|client| client.get(format!("{hub}/health/ready")).send())
                .map(|response| response.status().is_success())
                .unwrap_or(false);
            checks.push(json!({
                "id": "hub_reachability",
                "status": if reachable { "PASS" } else { "UNKNOWN" },
                "hub": hub,
            }));
        }
    }
    let registry = load_registry().unwrap_or_default();
    let roots: std::collections::BTreeSet<_> = registry
        .projects
        .values()
        .map(|entry| entry.root.clone())
        .collect();
    checks.push(json!({
        "id": "project_registry",
        "status": if roots.len() == registry.projects.len() { "PASS" } else { "FAIL" },
        "projects": registry.projects.len(),
        "identity_conflicts": registry.projects.len().saturating_sub(roots.len()),
        "registrations": registry.projects.values().map(|entry| json!({
            "project_id": entry.project_id,
            "instance_id": entry.instance_id,
            "registration": entry.hub_registration,
            "last_sync": entry.last_successful_hub_sync,
        })).collect::<Vec<_>>(),
    }));
    let current = std::env::current_dir()?;
    match discover_project(&current) {
        Ok(root) => {
            let (manifest, _) = load_project(&root)?;
            let entry = registry.projects.get(&manifest.project_id);
            let paths = entry.map(|value| project_paths(&root, &manifest, value.instance_id));
            let spool_files = paths
                .as_ref()
                .and_then(|value| value.spool_dir.read_dir().ok())
                .map(|items| {
                    items
                        .filter_map(Result::ok)
                        .filter(|item| item.file_type().is_ok_and(|kind| kind.is_file()))
                        .count()
                })
                .unwrap_or(0);
            let rejected_spool_reports = paths
                .as_ref()
                .and_then(|value| value.spool_dir.join("rejected").read_dir().ok())
                .map(|items| {
                    items
                        .filter_map(Result::ok)
                        .filter(|item| item.path().extension().is_some_and(|value| value == "json"))
                        .count()
                })
                .unwrap_or(0);
            let wiki_status = match manifest.wiki.mode.as_str() {
                "none" => manifest.wiki.binding_status.clone(),
                "local" if manifest.wiki.local_path.as_ref().is_some_and(|path| root.join(path).is_dir()) => "BOUND".into(),
                "local" | "remote" if manifest.wiki.wiki_id.is_none() && manifest.wiki.local_path.is_none() => "UNRESOLVED".into(),
                _ => "BOUND".into(),
            };
            checks.push(json!({
                "id": "local_project",
                "status": "PASS",
                "project_id": manifest.project_id,
                "enabled": manifest.enabled,
                "registration": entry.map(|value| value.hub_registration.as_str()).unwrap_or("PENDING"),
                "spool_files": spool_files,
                "pending_spool_files": spool_files,
                "rejected_spool_reports": rejected_spool_reports,
                "wiki": {"mode": manifest.wiki.mode, "binding_status": wiki_status},
            }));
        }
        Err(_) => checks.push(json!({
            "id": "local_project",
            "status": "NOT_ENABLED",
            "detail": "No enabled marker in the current directory; Code Intelligence remains inactive by design."
        })),
    }
    let skill = std::env::var_os("CODEINTEL_AGENT_HOME")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/root"))
        .join(".agents/skills/code-intelligence/SKILL.md");
    checks.push(json!({"id":"global_skill","status":if skill.is_file(){"PASS"}else{"UNKNOWN"},"path":skill}));
    checks.push(json!({
        "id": "worker_config",
        "status": if config.is_some() { "PASS" } else { "UNKNOWN" },
        "path": config_path(),
    }));
    checks.push(json!({
        "id": "worker_socket",
        "status": if socket.exists() { "PASS" } else { "UNKNOWN" },
        "path": socket,
    }));
    checks.push(json!({
        "id": "data_root",
        "status": if data_root().exists() { "PASS" } else { "UNKNOWN" },
        "path": data_root(),
    }));
    if let Some(client) = args.client {
        let adapter = adapter_status(&client);
        checks.push(json!({
            "id": format!("adapter:{client}"),
            "status": adapter.0,
            "detail": adapter.1,
        }));
    }
    let blocking = checks
        .iter()
        .filter(|item| item["status"] == "FAIL")
        .count();
    print_json(json!({
        "status": if blocking == 0 { "PASS_WITH_WARNINGS" } else { "FAIL" },
        "checks": checks,
        "blocking": blocking,
    }))
}

fn enroll(args: EnrollArgs) -> Result<()> {
    let host_name = fs::read_to_string("/etc/hostname")
        .unwrap_or_else(|_| "unknown-host".into())
        .trim()
        .to_string();
    let machine_id =
        fs::read_to_string("/etc/machine-id").unwrap_or_else(|_| Uuid::new_v4().to_string());
    let fingerprint = format!("{:x}", Sha256::digest(machine_id.trim().as_bytes()));
    let existing = load_config().ok();
    let host_id = existing
        .as_ref()
        .map(|config| config.host_id)
        .unwrap_or_else(Uuid::new_v4);
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .build()?;
    let response: Value = client
        .post(format!(
            "{}/api/v1/workers/enroll",
            args.hub.trim_end_matches('/')
        ))
        .json(&json!({
            "enrollment_token": args.token,
            "host_id": host_id,
            "host_name": host_name,
            "fingerprint": fingerprint,
            "os": std::env::consts::OS,
            "architecture": std::env::consts::ARCH,
            "worker_version": PRODUCT_VERSION,
            "protocol_version": 1,
            "adapters": args.clients,
        }))
        .send()?
        .error_for_status()?
        .json()?;
    let worker_id = Uuid::parse_str(
        response["worker_id"]
            .as_str()
            .ok_or_else(|| anyhow!("Hub enrollment response has no worker_id"))?,
    )?;
    let credential = response["worker_credential"]
        .as_str()
        .ok_or_else(|| anyhow!("Hub enrollment response has no credential"))?;
    let credential_path = PathBuf::from("/etc/code-intelligence/worker.credential");
    fs::create_dir_all(credential_path.parent().unwrap())?;
    codeintel_core::atomic_write(&credential_path, credential.as_bytes())?;
    fs::set_permissions(&credential_path, fs::Permissions::from_mode(0o600))?;
    let host_access = response["host_access_token"]
        .as_str()
        .ok_or_else(|| anyhow!("Hub enrollment response has no Host Access credential"))?;
    let host_access_path = data_root().join("credentials/host-access");
    fs::create_dir_all(host_access_path.parent().unwrap())?;
    codeintel_core::atomic_write(&host_access_path, host_access.as_bytes())?;
    fs::set_permissions(&host_access_path, fs::Permissions::from_mode(0o600))?;
    let config = WorkerConfig {
        schema_version: 1,
        hub_url: Some(args.hub.trim_end_matches('/').into()),
        worker_id: Some(worker_id),
        host_id,
        host_name,
        credential_file: credential_path,
        host_access_credential_file: Some(host_access_path),
        socket_path: PathBuf::from(DEFAULT_SOCKET),
        long_poll_sec: 25,
    };
    save_config(&config)?;
    fs::set_permissions(config_path(), fs::Permissions::from_mode(0o600))?;
    print_json(json!({
        "worker_id": worker_id,
        "host_id": host_id,
        "compatibility": response["compatibility"],
        "credential_stored": config.credential_file,
        "host_access_stored": config.host_access_credential_file,
        "credential_value_returned": false,
        "next": "Install/start the systemd unit and run codeintel doctor.",
    }))
}

fn bind_project_hub(args: HubBindArgs) -> Result<()> {
    if !args.hub.starts_with("https://") {
        return Err(anyhow!("Hub binding requires HTTPS"));
    }
    let root = discover_project(&args.path)?;
    let (manifest, _) = load_project(&root)?;
    let hub = args.hub.trim_end_matches('/');
    let response: Value = Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .build()?
        .get(format!("{hub}/api/v1/projects/{}", manifest.project_id))
        .bearer_auth(&args.project_token)
        .send()?
        .error_for_status()?
        .json()?;
    if response["project_id"].as_str() != Some(&manifest.project_id.to_string()) {
        return Err(anyhow!(
            "Project Access belongs to a different logical Project; use fork-project for a new clone"
        ));
    }
    configure_project_hub_binding(
        &root,
        Some(hub),
        "PROJECT_TOKEN",
        false,
        Some(&args.project_token),
    )?;
    let sync = request_value("sync_hub", json!({}))?;
    print_json(json!({
        "project_id": manifest.project_id,
        "hub": hub,
        "project_access": "PRESENT",
        "hub_registration": sync["registration"],
        "hub_sync": sync["sync"],
        "local_intelligence": "AVAILABLE",
    }))
}

fn recover_project(args: RecoverArgs) -> Result<()> {
    let root = fs::canonicalize(&args.path)?;
    if root.join(".code-intel/project.yaml").exists() {
        return Err(anyhow!(
            "Manifest already exists; recovery would overwrite portable identity"
        ));
    }
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .build()?;
    let project: Value = client
        .get(format!(
            "{}/api/v1/project-recovery-manifest",
            args.hub.trim_end_matches('/')
        ))
        .bearer_auth(args.token)
        .send()?
        .error_for_status()?
        .json()?;
    let project_id = project["project_id"]
        .as_str()
        .ok_or_else(|| anyhow!("Recovery response has no project_id"))?;
    let manifest = json!({
        "schema_version": 1,
        "project_id": project_id,
        "slug": project["slug"],
        "name": project["name"],
        "enabled": true,
        "source": {"primary_root": ".", "related_roots": [], "follow_symlinks": false, "git": {"mode": "metadata_optional"}},
        "wiki": {"mode": "none", "wiki_id": null, "local_path": null},
        "languages": {"auto_detect": true, "priority": ["php","typescript","javascript","python"]},
        "frameworks": {"auto_detect": true},
        "workspace": {"mode": "live"},
        "resources": {"profile": "ram_constrained", "max_heavy_jobs": 1, "max_lsp_processes": 1, "project_soft_mb": 768, "project_hard_mb": 1536}
    });
    fs::create_dir_all(root.join(".code-intel"))?;
    codeintel_core::atomic_write(
        &root.join(".code-intel/project.yaml"),
        serde_yaml::to_string(&manifest)?.as_bytes(),
    )?;
    codeintel_core::atomic_write(
        &root.join(".code-intel/policy.yaml"),
        serde_yaml::to_string(&codeintel_core::Policy::default())?.as_bytes(),
    )?;
    codeintel_core::atomic_write(
        &root.join(".code-intel/AGENT_POLICY.md"),
        b"# Project-specific Code Intelligence policy\n\nRecovered identity; no project-specific additions.\n",
    )?;
    let recovered_manifest: ProjectManifest = serde_json::from_value(manifest)?;
    write_portable_project_files(&root, &recovered_manifest)?;
    let project_access = project["project_access_token"]
        .as_str()
        .ok_or_else(|| anyhow!("Recovery response has no Project Access"))?;
    configure_project_hub_binding(
        &root,
        Some(args.hub.trim_end_matches('/')),
        "RECOVERED",
        false,
        Some(project_access),
    )?;
    let opened = request_value("open_project", json!({"path": root}))?;
    let sync = request_value("sync_hub", json!({}))?;
    print_json(json!({
        "project_id": opened["project_id"],
        "instance_id": opened["instance_id"],
        "hub_registration": sync["registration"],
        "hub_sync": sync["sync"],
        "project_access": "PRESENT",
        "local_intelligence": "READY"
    }))
}

fn adapter_status(client: &str) -> (&'static str, String) {
    let home = std::env::var_os("CODEINTEL_AGENT_HOME")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/root"));
    let candidates = match client {
        "codex" => vec![
            home.join(".codex/config.toml"),
            home.join(".config/codex/config.toml"),
        ],
        "claude" => vec![
            home.join(".claude.json"),
            home.join(".config/claude/config.json"),
        ],
        "hermes" => vec![
            home.join(".hermes/config.yaml"),
            home.join(".config/hermes/config.yaml"),
        ],
        _ => vec![],
    };
    if let Some(path) = candidates.into_iter().find(|path| path.exists()) {
        ("PASS", format!("Detected {}", path.display()))
    } else {
        (
            "UNKNOWN",
            "Client config not detected; run the installer adapter.".into(),
        )
    }
}

fn selected_wiki(root: &std::path::Path, args: &InitArgs) -> Result<WikiConfig> {
    let Some(mode) = args.wiki_mode.as_deref() else {
        return Ok(discover_wiki(root));
    };
    match mode {
        "none" => Ok(WikiConfig::default()),
        "local" => {
            let path = args
                .wiki_path
                .as_ref()
                .ok_or_else(|| anyhow!("--wiki-path is required with --wiki-mode local"))?;
            Ok(WikiConfig {
                mode: "local".into(),
                wiki_id: args.wiki_id.clone(),
                local_path: Some(path.to_string_lossy().to_string()),
                binding_status: if root.join(path).is_dir() {
                    "BOUND"
                } else {
                    "UNAVAILABLE"
                }
                .into(),
            })
        }
        "remote" => Ok(WikiConfig {
            mode: "remote".into(),
            wiki_id: args.wiki_id.clone(),
            local_path: None,
            binding_status: if args.wiki_id.is_some() {
                "BOUND"
            } else {
                "UNRESOLVED"
            }
            .into(),
        }),
        _ => Err(anyhow!("Unsupported Wiki mode")),
    }
}

fn print_json(value: Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn slugify(value: &str) -> String {
    let mut output = String::new();
    let mut dash = false;
    for character in value.to_lowercase().chars() {
        if character.is_ascii_alphanumeric() {
            output.push(character);
            dash = false;
        } else if !dash && !output.is_empty() {
            output.push('-');
            dash = true;
        }
    }
    output.trim_matches('-').to_string()
}

fn classify_error(message: &str) -> &str {
    if message.contains("IN_USE") {
        "IN_USE"
    } else if message.contains("SOURCE_DRIFT") {
        "SOURCE_DRIFT"
    } else if message.contains("No .code-intel") {
        "PROJECT_NOT_FOUND"
    } else if message.contains("RECOVERY_REQUIRED") {
        "RECOVERY_REQUIRED"
    } else {
        "COMMAND_FAILED"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn relative_cli_path_is_resolved_before_it_reaches_daemon() {
        let directory = tempdir().unwrap();
        let project = directory.path().join("project");
        fs::create_dir(&project).unwrap();
        assert_eq!(
            resolve_request_path(std::path::Path::new("project"), directory.path()).unwrap(),
            fs::canonicalize(project).unwrap()
        );
    }
}
