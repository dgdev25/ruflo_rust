use std::ffi::OsString;

/// Stable error code emitted when an invocation is outside the native CLI
/// surface. Consumers can match this prefix without parsing human guidance.
pub const UNSUPPORTED_COMMAND_ERROR_CODE: &str = "cli.unsupported";

#[derive(Debug, Clone, PartialEq)]
pub enum ParsedCommand {
    Version,
    VersionCommand {
        explain: bool,
        require_catalog_gte: Option<u64>,
    },
    Completions {
        shell: String,
    },
    CompletionsOverview,
    Doctor,
    Start {
        topology: String,
        daemon: bool,
    },
    Progress,
    Cleanup {
        force: bool,
        keep_config: bool,
    },
    CleanupHelp,
    TransportOverview,
    TransportHelp,
    TransportUseHelp,
    TransportUse {
        name: Option<String>,
        quiet: bool,
    },
    Deployment(crate::deployment::DeploymentCommand),
    Claims(crate::claims::ClaimsCommand),
    Advisor(crate::funnel::AdvisorCommand),
    Announcements(crate::announcements::AnnouncementsCommand),
    Spinner(crate::spinner::SpinnerCommand),
    Settings(crate::settings::SettingsCommand),
    Funnel(crate::funnel_command::FunnelCommand),
    Eject(crate::eject::EjectCommand),
    Issues(crate::issues::IssuesCommand),
    Benchmark(crate::benchmark::BenchmarkCommand),
    MetaHarness(crate::metaharness::MetaCommand),
    Verify(crate::verify::VerifyCommand),
    Policy(crate::policy::PolicyCommand),
    UpdateCmd(crate::update_cmd::UpdateCommand),
    Providers(crate::providers::ProvidersCommand),
    Auth(crate::auth::AuthCommand),
    Autopilot(crate::autopilot::AutopilotCommand),
    Proxy(crate::proxy::ProxyCommand),
    ApplianceAdvanced(crate::appliance_advanced::ApplianceAdvancedCommand),
    Appliance(crate::appliance::ApplianceCommand),
    Guidance(crate::guidance::GuidanceCommand),
    Performance(crate::performance::PerformanceCommand),
    GaiaBench(crate::gaia_bench::GaiaBenchCommand),
    ProcessCmd(crate::process_cmd::ProcessCommand),
    Workflow(crate::workflow::WorkflowCommand),
    Route(crate::route::RouteCommand),
    Plugins(crate::plugins::PluginsCommand),
    Security(crate::security::SecurityCommand),
    Analyze(crate::analyze::AnalyzeCommand),
    Daemon(crate::daemon::DaemonCommand),
    Embeddings(crate::embeddings::EmbeddingsCommand),
    HiveMind(crate::hive_mind::HiveMindCommand),
    Neural(crate::neural::NeuralCommand),
    Hooks(crate::hooks::HooksCommand),
    TransferStore(crate::transfer_store::TransferStoreCommand),
    MemoryStore {
        key: String,
        value: String,
        namespace: String,
        tags_json: Option<String>,
        provenance_type: String,
        upsert: bool,
        path: Option<String>,
    },
    MemoryRetrieve {
        key: String,
        namespace: String,
        value_only: bool,
        path: Option<String>,
    },
    MemorySearch {
        query: String,
        namespace: Option<String>,
        limit: usize,
        path: Option<String>,
    },
    MemoryList {
        namespace: Option<String>,
        limit: usize,
        path: Option<String>,
    },
    MemoryDelete {
        key: String,
        namespace: String,
        path: Option<String>,
    },
    MemoryStats {
        path: Option<String>,
    },
    MemoryRebuildIndex {
        path: Option<String>,
    },
    MemoryMigrateNode {
        path: Option<String>,
        dry_run: bool,
    },
    MemoryBackup {
        path: Option<String>,
    },
    MemoryDistill {
        path: Option<String>,
    },
    MemoryPurge {
        namespace: String,
        dry_run: bool,
        force: bool,
        path: Option<String>,
    },
    ConfigInit {
        force: bool,
        sparc: bool,
        v3: bool,
    },
    ConfigGet {
        key: Option<String>,
        json: bool,
    },
    ConfigSet {
        key: String,
        value: String,
    },
    ConfigProviders {
        add: Option<String>,
        remove: Option<String>,
        enable: Option<String>,
        disable: Option<String>,
        json: bool,
    },
    ConfigReset {
        force: bool,
        section: Option<String>,
    },
    ConfigExport {
        output: String,
        format: String,
    },
    ConfigImport {
        file: String,
        merge: bool,
    },
    ConfigOverview,
    ConfigHelp {
        subcommand: Option<String>,
    },
    MigrateStatus,
    MigrateRun {
        target: String,
        dry_run: bool,
        backup: bool,
        force: bool,
    },
    Help,
    Init,
    Status,
    SwarmInit {
        topology: String,
        max_agents: usize,
        strategy: String,
    },
    SwarmStatus,
    SwarmStart {
        objective: String,
        strategy: String,
        workers: usize,
        agent: String,
        dry_run: bool,
        keep_env: bool,
        worktree: bool,
    },
    SwarmStop {
        swarm_id: String,
    },
    SwarmScale {
        swarm_id: String,
        target_agents: usize,
        agent_type: Option<String>,
    },
    SwarmCoordinate {
        agents: usize,
    },
    SwarmCompressMessage {
        message: Option<String>,
        message_file: Option<String>,
        budget_tokens: usize,
        mode: String,
    },
    SessionSave {
        name: String,
        description: String,
    },
    SessionList,
    SessionRestore {
        session_id: String,
    },
    SessionDelete {
        session_id: String,
    },
    SessionExport {
        session_id: Option<String>,
        output: String,
    },
    SessionImport {
        input: String,
        name: Option<String>,
    },
    SessionCurrent,
    AgentSpawn {
        agent_type: String,
        name: String,
    },
    AgentList,
    AgentStatus {
        agent_id: String,
    },
    AgentStop {
        agent_id: String,
        force: bool,
        timeout_seconds: u64,
    },
    AgentMetrics {
        agent_id: Option<String>,
        period: String,
    },
    AgentPool {
        size: Option<usize>,
        min: usize,
        max: usize,
        auto_scale: bool,
    },
    AgentHealth {
        agent_id: Option<String>,
        detailed: bool,
    },
    AgentLogs {
        agent_id: String,
        tail: usize,
        level: String,
        follow: bool,
        since: Option<String>,
    },
    TaskCreate {
        task_type: String,
        description: String,
        priority: String,
    },
    TaskList,
    TaskStatus {
        task_id: String,
    },
    TaskCancel {
        task_id: String,
        reason: String,
    },
    TaskAssign {
        task_id: String,
        agent_ids: Vec<String>,
        unassign: bool,
    },
    TaskRetry {
        task_id: String,
        reset_state: bool,
    },
    McpStart,
    McpOp {
        op: String,
    },
    WasmOp {
        op: String,
    },
    RuvectorOp {
        op: String,
    },
    NativeOverview {
        name: String,
    },
}

