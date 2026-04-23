mod offline_adapter_sdk;

use offline_adapter_sdk::{FunctionOfflineAdapter, OfflineAdapterRegistry};
use regex::Regex;
use reqwest::blocking::Client;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Manager;
use url::Url;
use walkdir::WalkDir;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScanRequest {
    root_path: String,
    strict_offline: bool,
    allow_official_api_exceptions: bool,
    #[serde(default)]
    settings: ScanSettings,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct ScanSettings {
    #[serde(default)]
    game_paths: BTreeMap<String, String>,
    #[serde(default)]
    manual_fallback_paths: BTreeMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ScanFinding {
    game_id: String,
    source_file: String,
    kind: String,
    value: String,
    #[serde(skip_serializing, skip_deserializing, default)]
    raw_value: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct GameScanResult {
    game_id: String,
    status: String,
    completeness: String,
    detected_files: usize,
    findings_count: usize,
    note: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanResponse {
    scan_id: i64,
    scanned_at: String,
    root_path: String,
    strict_offline: bool,
    allow_official_api_exceptions: bool,
    network_policy: NetworkPolicyState,
    game_results: Vec<GameScanResult>,
    findings: Vec<ScanFinding>,
    history_pulls: Vec<HistoryPullRow>,
    notes: Vec<String>,
    troubleshooting: Vec<GameTroubleshooting>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanSummary {
    id: i64,
    scanned_at: String,
    root_path: String,
    strict_offline: bool,
    allow_official_api_exceptions: bool,
    total_findings: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Pull {
    id: String,
    game_id: String,
    banner_id: String,
    source_file: String,
    source_type: String,
    kind: String,
    value: String,
    item_name: Option<String>,
    item_type_name: Option<String>,
    rarity: Option<i64>,
    pulled_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Banner {
    id: String,
    game_id: String,
    name: String,
    pull_type: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SourceMetadata {
    id: String,
    game_id: String,
    path: String,
    source_type: String,
    findings: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ImportDiagnostic {
    id: String,
    severity: String,
    message: String,
    game_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ImportSession {
    id: i64,
    scanned_at: String,
    root_path: String,
    strict_offline: bool,
    allow_official_api_exceptions: bool,
    game_results: Vec<GameScanResult>,
    completeness_by_game: BTreeMap<String, String>,
    pulls: Vec<Pull>,
    banners: Vec<Banner>,
    source_metadata: Vec<SourceMetadata>,
    diagnostics: Vec<ImportDiagnostic>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunLocalImportResponse {
    scan: ScanResponse,
    import_session: ImportSession,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct HistoryPullRow {
    game_id: String,
    banner_id: String,
    banner_name: String,
    item_name: String,
    item_type_name: String,
    rarity: i64,
    pulled_at: String,
    pull_id: String,
    source_url: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NetworkPolicyState {
    mode: String,
    online_calls_allowed: bool,
    approved_hosts: Vec<String>,
    approved_candidates: usize,
    blocked_candidates: usize,
    message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct GameTroubleshooting {
    game_id: String,
    effective_root_path: String,
    root_override_applied: bool,
    configured_manual_fallback_path: Option<String>,
    manual_fallback_status: String,
    checked_path_hints: Vec<String>,
    detected_source_files: Vec<String>,
    status: String,
    completeness: String,
    note: String,
    next_steps: Vec<String>,
}

#[derive(Debug, Clone)]
struct TroubleshootingDraft {
    game_id: String,
    effective_root_path: String,
    root_override_applied: bool,
    configured_manual_fallback_path: Option<String>,
    manual_fallback_status: String,
    checked_path_hints: Vec<String>,
    detected_source_files: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
struct NetworkPolicy {
    strict_offline: bool,
    allow_official_api_exceptions: bool,
}

const CONFIRMED_OFFICIAL_API_HOSTS: [&str; 8] = [
    "gmserver-api.aki-game2.com",
    "gmserver-api.aki-game2.net",
    "public-operation-hkrpg.mihoyo.com",
    "public-operation-hkrpg.hoyoverse.com",
    "public-operation-hkrpg-sg.hoyoverse.com",
    "public-operation-hkrpg-os.hoyoverse.com",
    "public-operation-nap.hoyoverse.com",
    "public-operation-nap-sg.hoyoverse.com",
];
const DB_SCHEMA_VERSION: i64 = 5;
const WUWA_ROOT_DIR_HINTS: [&str; 4] = [
    "WutheringWaves",
    "Wuthering Waves",
    "WutheringWaves Game",
    "Wuthering Waves Game",
];
const HSR_ROOT_DIR_HINTS: [&str; 3] = ["HonkaiStarrail", "HonkaiStarRail", "Honkai Star Rail"];
const ZZZ_ROOT_DIR_HINTS: [&str; 3] = ["ZZZ", "ZenlessZoneZero", "Zenless Zone Zero"];
const ENDFIELD_ROOT_DIR_HINTS: [&str; 2] = ["Endfield", "EndField"];

impl NetworkPolicy {
    fn from_request(request: &ScanRequest) -> Result<Self, String> {
        if request.strict_offline && request.allow_official_api_exceptions {
            return Err(
                "Official API exceptions require strict offline mode to be disabled first."
                    .to_string(),
            );
        }
        Ok(Self {
            strict_offline: request.strict_offline,
            allow_official_api_exceptions: request.allow_official_api_exceptions,
        })
    }

    fn exception_requested(&self) -> bool {
        !self.strict_offline && self.allow_official_api_exceptions
    }

    fn confirmed_hosts_available(&self) -> bool {
        !CONFIRMED_OFFICIAL_API_HOSTS.is_empty()
    }

    fn online_calls_allowed(&self) -> bool {
        self.exception_requested() && self.confirmed_hosts_available()
    }

    fn mode(&self) -> &'static str {
        if self.online_calls_allowed() {
            "official-api-exceptions"
        } else {
            "strict-local-only"
        }
    }

    fn status_message(&self) -> String {
        if self.online_calls_allowed() {
            "Online exception path is enabled. Only officially confirmed API hosts are eligible."
                .to_string()
        } else if self.exception_requested() && !self.confirmed_hosts_available() {
            "Official API exception was requested, but no officially confirmed hosts are configured. Runtime remains strict local-only."
                .to_string()
        } else {
            "Strict local-only mode is active. All online calls are blocked at runtime.".to_string()
        }
    }

    fn approved_hosts(&self) -> Vec<String> {
        CONFIRMED_OFFICIAL_API_HOSTS
            .iter()
            .map(|host| host.to_string())
            .collect()
    }

    fn assert_url_allowed(&self, url: &str) -> Result<(), String> {
        if !self.online_calls_allowed() {
            return Err("Blocked by strict local-only policy.".to_string());
        }
        let host = extract_url_host(url)
            .ok_or_else(|| format!("Blocked URL without a valid host: {url}"))?;
        if is_approved_official_api_host(&host) {
            Ok(())
        } else {
            Err(format!("Blocked non-approved host: {host}"))
        }
    }
}

#[tauri::command]
async fn run_local_scan(
    app: tauri::AppHandle,
    request: ScanRequest,
) -> Result<ScanResponse, String> {
    tauri::async_runtime::spawn_blocking(move || execute_local_scan(&app, &request))
        .await
        .map_err(|e| format!("Scan task failed: {e}"))?
}

#[tauri::command]
async fn run_local_import(
    app: tauri::AppHandle,
    request: ScanRequest,
) -> Result<RunLocalImportResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let scan = execute_local_scan(&app, &request)?;
        let import_session = build_import_session(&scan);
        let db_path = database_path(&app)?;
        let mut conn = open_db(&db_path)?;
        persist_import_session(&mut conn, &import_session)?;
        recompute_import_summary(&conn, import_session.id)?;
        let recomputed = load_import_session(&conn, import_session.id)?;

        Ok(RunLocalImportResponse {
            scan,
            import_session: recomputed,
        })
    })
    .await
    .map_err(|e| format!("Import task failed: {e}"))?
}

fn execute_local_scan(
    app: &tauri::AppHandle,
    request: &ScanRequest,
) -> Result<ScanResponse, String> {
    let policy = NetworkPolicy::from_request(&request)?;
    let requested_root = request.root_path.trim();
    let root_path = if requested_root.is_empty() {
        None
    } else {
        Some(PathBuf::from(requested_root))
    };
    let scan_root_label = if requested_root.is_empty() {
        "<auto-detect>".to_string()
    } else {
        requested_root.to_string()
    };

    let mut all_findings = Vec::<ScanFinding>::new();
    let mut game_results = Vec::<GameScanResult>::new();
    let mut troubleshooting_drafts = Vec::<TroubleshootingDraft>::new();
    let mut notes = vec![policy.status_message()];

    for adapter in build_offline_adapter_registry().iter() {
        let adapter_id = adapter.id();
        let (effective_root, root_override_applied) =
            resolve_effective_root(root_path.as_deref(), &request.settings, adapter_id);
        let mut scan = adapter.scan(&effective_root)?;
        if scan.game_result.game_id != adapter_id {
            return Err(format!(
                "Offline adapter '{adapter_id}' returned mismatched game_id '{}'.",
                scan.game_result.game_id
            ));
        }

        let configured_manual_fallback_path =
            configured_manual_fallback_path(&request.settings, adapter_id);
        let mut manual_fallback_status = "not-configured".to_string();
        let mut detected_source_files = scan
            .findings
            .iter()
            .map(|finding| finding.source_file.clone())
            .collect::<Vec<_>>();

        if let Some(path) = configured_manual_fallback_path.as_ref() {
            let manual_path = PathBuf::from(path);
            if manual_path.exists() && manual_path.is_file() {
                match read_text(&manual_path) {
                    Ok(text) => {
                        manual_fallback_status = "loaded".to_string();
                        let mut fallback_findings =
                            extract_gacha_log_findings(&text, adapter_id, &manual_path)?;
                        if !fallback_findings.is_empty() {
                            notes.push(format!(
                                "Manual fallback for {adapter_id} produced {} additional finding(s).",
                                fallback_findings.len()
                            ));
                        }
                        scan.findings.append(&mut fallback_findings);
                        detected_source_files.push(manual_path.display().to_string());
                        if scan.game_result.detected_files == 0 {
                            scan.game_result.detected_files = 1;
                        }
                    }
                    Err(_) => {
                        manual_fallback_status = "read-error".to_string();
                        notes.push(format!(
                            "Manual fallback file for {adapter_id} could not be read: {}",
                            manual_path.display()
                        ));
                    }
                }
            } else {
                manual_fallback_status = "missing".to_string();
                notes.push(format!(
                    "Manual fallback file for {adapter_id} does not exist: {}",
                    manual_path.display()
                ));
            }
        }

        dedup_findings(&mut scan.findings);
        dedup_strings(&mut detected_source_files);
        all_findings.append(&mut scan.findings);
        game_results.push(scan.game_result);
        troubleshooting_drafts.push(TroubleshootingDraft {
            game_id: adapter_id.to_string(),
            effective_root_path: effective_root.display().to_string(),
            root_override_applied,
            configured_manual_fallback_path,
            manual_fallback_status,
            checked_path_hints: build_checked_path_hints(adapter_id, &effective_root),
            detected_source_files,
        });
    }
    dedup_findings(&mut all_findings);
    recompute_game_results_from_findings(&mut game_results, &all_findings);
    let mut history_pulls = Vec::<HistoryPullRow>::new();
    if policy.online_calls_allowed() {
        match fetch_history_pulls_from_tokens(&all_findings, &policy) {
            Ok(rows) => {
                history_pulls = rows;
                apply_history_pulls_to_game_results(&mut game_results, &history_pulls);
                if !history_pulls.is_empty() {
                    notes.push(format!(
                        "Fetched {} pull-history rows from official game endpoints.",
                        history_pulls.len()
                    ));
                }
            }
            Err(err) => {
                notes.push(format!(
                    "Official API history fetch failed ({err}). Showing local findings only."
                ));
            }
        }
    } else {
        notes.push(
            "Full pull history fetch is disabled by policy. Enable official API exceptions to load complete history."
                .to_string(),
        );
    }
    let troubleshooting = build_game_troubleshooting(&game_results, &troubleshooting_drafts);

    let network_policy = build_network_policy_state(&policy, &all_findings);
    notes.push(network_policy.message.clone());
    if network_policy.online_calls_allowed {
        notes.push(
            "Officially confirmed-host exception path is available for official APIs, but this scan pipeline remains local-only."
                .to_string(),
        );
    } else if policy.exception_requested() && !policy.confirmed_hosts_available() {
        notes.push(
            "Exception mode was requested, but no officially confirmed API hosts are configured yet."
                .to_string(),
        );
    }

    notes.push(
        "If a game only exposes URL tokens locally, data completeness will be marked as partial."
            .to_string(),
    );

    let scanned_at = unix_ts().to_string();
    let db_path = database_path(&app)?;
    let mut conn = open_db(&db_path)?;
    let scan_id = persist_scan(
        &mut conn,
        &scanned_at,
        &request,
        &scan_root_label,
        &game_results,
        &all_findings,
    )?;

    Ok(ScanResponse {
        scan_id,
        scanned_at,
        root_path: scan_root_label,
        strict_offline: request.strict_offline,
        allow_official_api_exceptions: request.allow_official_api_exceptions,
        network_policy,
        game_results,
        findings: all_findings,
        history_pulls,
        notes,
        troubleshooting,
    })
}

fn build_offline_adapter_registry() -> OfflineAdapterRegistry {
    let mut registry = OfflineAdapterRegistry::new();
    registry.register(FunctionOfflineAdapter::new("wuthering-waves", scan_wuwa));
    registry.register(FunctionOfflineAdapter::new("honkai-star-rail", scan_hsr));
    registry.register(FunctionOfflineAdapter::new("zenless-zone-zero", scan_zzz));
    registry.register(FunctionOfflineAdapter::new("endfield", scan_endfield));
    registry
}

#[tauri::command]
fn get_recent_scans(app: tauri::AppHandle, limit: Option<u32>) -> Result<Vec<ScanSummary>, String> {
    let db_path = database_path(&app)?;
    let conn = open_db(&db_path)?;
    let take = limit.unwrap_or(15).clamp(1, 100);

    let mut stmt = conn
        .prepare(
            "
            SELECT s.id, s.scanned_at, s.root_path, s.strict_offline, s.allow_official_api_exceptions, 
                   COALESCE((SELECT COUNT(*) FROM findings f WHERE f.scan_id = s.id), 0) as total_findings
            FROM scans s
            ORDER BY s.id DESC
            LIMIT ?1
            ",
        )
        .map_err(|e| format!("Failed querying scans: {e}"))?;

    let rows = stmt
        .query_map(params![take], |row| {
            Ok(ScanSummary {
                id: row.get(0)?,
                scanned_at: row.get(1)?,
                root_path: row.get(2)?,
                strict_offline: row.get::<_, i64>(3)? == 1,
                allow_official_api_exceptions: row.get::<_, i64>(4)? == 1,
                total_findings: row.get(5)?,
            })
        })
        .map_err(|e| format!("Failed mapping scan rows: {e}"))?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| format!("Failed reading scan row: {e}"))?);
    }
    Ok(out)
}

#[tauri::command]
fn get_latest_import_session(app: tauri::AppHandle) -> Result<Option<ImportSession>, String> {
    let db_path = database_path(&app)?;
    let conn = open_db(&db_path)?;
    let latest_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM import_sessions ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("Failed querying latest import session: {e}"))?;

    if let Some(scan_id) = latest_id {
        let session = load_import_session(&conn, scan_id)?;
        Ok(Some(session))
    } else {
        Ok(None)
    }
}

fn source_type_from_path(path: &str) -> &'static str {
    let normalized = path.to_ascii_lowercase();
    if normalized.contains("cache") || normalized.ends_with("data_2") {
        "cache"
    } else if normalized.ends_with(".log") {
        "log"
    } else {
        "unknown"
    }
}

fn banner_name_by_game(game_id: &str) -> &'static str {
    match game_id {
        "wuthering-waves" => "Convene",
        "honkai-star-rail" => "Warp",
        "zenless-zone-zero" => "Signal Search",
        "endfield" => "Recruitment",
        _ => "Unknown banner",
    }
}

fn banner_name_from_banner_id(game_id: &str, banner_id: &str) -> String {
    let suffix = banner_id.split(':').nth(1).unwrap_or_default();
    if game_id == "wuthering-waves" {
        if let Ok(card_pool_type) = suffix.parse::<i64>() {
            return wuwa_banner_name(card_pool_type).to_string();
        }
        return banner_name_by_game(game_id).to_string();
    }
    if game_id == "honkai-star-rail" || game_id == "zenless-zone-zero" {
        return hoyo_banner_name(game_id, suffix);
    }
    banner_name_by_game(game_id).to_string()
}

fn banner_pull_type_from_kind(kind: &str) -> &'static str {
    if kind == "possible_history_source" {
        "possible-source"
    } else if kind == "history_pull" {
        "history-pull"
    } else {
        "history-url"
    }
}

fn banner_id_for(game_id: &str, kind: &str) -> String {
    format!("{game_id}:{}", banner_pull_type_from_kind(kind))
}

fn build_import_session(scan: &ScanResponse) -> ImportSession {
    let pulls: Vec<Pull> = if !scan.history_pulls.is_empty() {
        scan.history_pulls
            .iter()
            .enumerate()
            .map(|(index, row)| Pull {
                id: if row.pull_id.is_empty() {
                    format!("{}:{index}", scan.scan_id)
                } else {
                    row.pull_id.clone()
                },
                game_id: row.game_id.clone(),
                banner_id: row.banner_id.clone(),
                source_file: row.source_url.clone(),
                source_type: "network".to_string(),
                kind: "history_pull".to_string(),
                value: row.item_name.clone(),
                item_name: Some(row.item_name.clone()),
                item_type_name: Some(row.item_type_name.clone()),
                rarity: Some(row.rarity),
                pulled_at: Some(row.pulled_at.clone()),
            })
            .collect()
    } else {
        scan.findings
            .iter()
            .enumerate()
            .map(|(index, finding)| Pull {
                id: format!("{}:{index}", scan.scan_id),
                game_id: finding.game_id.clone(),
                banner_id: banner_id_for(&finding.game_id, &finding.kind),
                source_file: finding.source_file.clone(),
                source_type: source_type_from_path(&finding.source_file).to_string(),
                kind: finding.kind.clone(),
                value: finding.value.clone(),
                item_name: None,
                item_type_name: None,
                rarity: None,
                pulled_at: None,
            })
            .collect()
    };

    let mut banners_by_id = BTreeMap::<String, Banner>::new();
    for pull in &pulls {
        banners_by_id
            .entry(pull.banner_id.clone())
            .or_insert(Banner {
                id: pull.banner_id.clone(),
                game_id: pull.game_id.clone(),
                name: banner_name_from_banner_id(&pull.game_id, &pull.banner_id),
                pull_type: banner_pull_type_from_kind(&pull.kind).to_string(),
            });
    }

    for game in &scan.game_results {
        let default_banner_id = format!("{}:history-url", game.game_id);
        banners_by_id
            .entry(default_banner_id.clone())
            .or_insert(Banner {
                id: default_banner_id,
                game_id: game.game_id.clone(),
                name: banner_name_by_game(&game.game_id).to_string(),
                pull_type: "history-url".to_string(),
            });
    }

    let mut source_metadata_by_id = BTreeMap::<String, SourceMetadata>::new();
    for pull in &pulls {
        let id = format!("{}:{}", pull.game_id, pull.source_file);
        if let Some(existing) = source_metadata_by_id.get_mut(&id) {
            existing.findings += 1;
            continue;
        }
        source_metadata_by_id.insert(
            id.clone(),
            SourceMetadata {
                id,
                game_id: pull.game_id.clone(),
                path: pull.source_file.clone(),
                source_type: pull.source_type.clone(),
                findings: 1,
            },
        );
    }

    let mut diagnostics: Vec<ImportDiagnostic> = scan
        .notes
        .iter()
        .enumerate()
        .map(|(index, note)| ImportDiagnostic {
            id: format!("note:{index}"),
            severity: "info".to_string(),
            message: note.clone(),
            game_id: None,
        })
        .collect();

    diagnostics.extend(
        scan.game_results
            .iter()
            .filter(|game| game.status != "ready")
            .map(|game| ImportDiagnostic {
                id: format!("status:{}", game.game_id),
                severity: "warning".to_string(),
                message: game.note.clone(),
                game_id: Some(game.game_id.clone()),
            }),
    );

    if pulls.is_empty() {
        diagnostics.push(ImportDiagnostic {
            id: "summary:no-pulls".to_string(),
            severity: "warning".to_string(),
            message: "No pull-history findings were extracted from local artifacts.".to_string(),
            game_id: None,
        });
    }

    let completeness_by_game = scan
        .game_results
        .iter()
        .map(|game| (game.game_id.clone(), game.completeness.clone()))
        .collect::<BTreeMap<_, _>>();

    ImportSession {
        id: scan.scan_id,
        scanned_at: scan.scanned_at.clone(),
        root_path: scan.root_path.clone(),
        strict_offline: scan.strict_offline,
        allow_official_api_exceptions: scan.allow_official_api_exceptions,
        game_results: scan.game_results.clone(),
        completeness_by_game,
        pulls,
        banners: banners_by_id.into_values().collect(),
        source_metadata: source_metadata_by_id.into_values().collect(),
        diagnostics,
    }
}

fn resolve_effective_root(
    root_path: Option<&Path>,
    settings: &ScanSettings,
    game_id: &str,
) -> (PathBuf, bool) {
    if let Some(value) = settings.game_paths.get(game_id) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return (PathBuf::from(trimmed), true);
        }
    }
    if let Some(path) = root_path {
        return (path.to_path_buf(), false);
    }
    (auto_detect_effective_root(game_id), false)
}

fn default_scan_root() -> PathBuf {
    if let Ok(system_drive) = std::env::var("SystemDrive") {
        let drive_root = PathBuf::from(format!(r"{system_drive}\"));
        if drive_root.exists() {
            return drive_root;
        }
    }
    PathBuf::from(r"C:\")
}

fn available_drive_roots() -> Vec<PathBuf> {
    let mut roots = Vec::<PathBuf>::new();
    let mut seen = HashSet::<String>::new();

    let mut push_root = |path: PathBuf| {
        let normalized = normalize_path_key(&path);
        if path.exists() && path.is_dir() && seen.insert(normalized) {
            roots.push(path);
        }
    };

    push_root(default_scan_root());
    for letter in 'A'..='Z' {
        push_root(PathBuf::from(format!(r"{letter}:\\")));
    }

    if roots.is_empty() {
        roots.push(default_scan_root());
    }
    roots
}

fn game_artifacts_exist_for_root(game_id: &str, root: &Path) -> bool {
    if game_id == "wuthering-waves" {
        return build_wuwa_log_candidates(root)
            .into_iter()
            .any(|candidate| candidate.exists() && candidate.is_file());
    }

    let (game_root_hints, files): (&[&str], &[&str]) = match game_id {
        "honkai-star-rail" => (
            &HSR_ROOT_DIR_HINTS,
            &[
                "Player.log",
                "Player-prev.log",
                "webCaches\\Cache\\Cache_Data\\data_2",
            ],
        ),
        "zenless-zone-zero" => (
            &ZZZ_ROOT_DIR_HINTS,
            &[
                "Player.log",
                "Player-prev.log",
                "webCaches\\Cache\\Cache_Data\\data_2",
            ],
        ),
        "endfield" => (
            &ENDFIELD_ROOT_DIR_HINTS,
            &[
                "Player.log",
                "Player-prev.log",
                "webCaches\\Cache\\Cache_Data\\data_2",
            ],
        ),
        _ => (&[], &[]),
    };

    for game_root in game_root_hints {
        for file in files {
            if root.join(game_root).join(file).exists() {
                return true;
            }
        }
    }
    for file in files {
        if root.join(file).exists() {
            return true;
        }
    }
    false
}

fn auto_detect_effective_root(game_id: &str) -> PathBuf {
    for root in available_drive_roots() {
        if game_artifacts_exist_for_root(game_id, &root) {
            return root;
        }
    }
    default_scan_root()
}

fn configured_manual_fallback_path(settings: &ScanSettings, game_id: &str) -> Option<String> {
    settings
        .manual_fallback_paths
        .get(game_id)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn build_checked_path_hints(game_id: &str, effective_root: &Path) -> Vec<String> {
    let mut hints = Vec::<String>::new();

    let game_root_hints: &[&str] = match game_id {
        "wuthering-waves" => &WUWA_ROOT_DIR_HINTS,
        "honkai-star-rail" => &HSR_ROOT_DIR_HINTS,
        "zenless-zone-zero" => &ZZZ_ROOT_DIR_HINTS,
        "endfield" => &ENDFIELD_ROOT_DIR_HINTS,
        _ => &[],
    };

    for root_hint in game_root_hints {
        hints.push(effective_root.join(root_hint).display().to_string());
    }

    match game_id {
        "wuthering-waves" => {
            for path in build_wuwa_log_candidates(effective_root)
                .into_iter()
                .take(6)
            {
                hints.push(path.display().to_string());
            }
        }
        _ => {
            hints.push(effective_root.join("Player.log").display().to_string());
            hints.push(effective_root.join("Player-prev.log").display().to_string());
        }
    }

    hints.push(
        effective_root
            .join("webCaches")
            .join("Cache")
            .join("Cache_Data")
            .join("data_2")
            .display()
            .to_string(),
    );

    dedup_strings(&mut hints);
    hints.truncate(8);
    hints
}

fn dedup_strings(values: &mut Vec<String>) {
    let mut seen = HashSet::<String>::new();
    values.retain(|value| seen.insert(value.to_ascii_lowercase()));
}

fn recompute_game_results_from_findings(
    game_results: &mut [GameScanResult],
    findings: &[ScanFinding],
) {
    let mut findings_by_game = BTreeMap::<String, usize>::new();
    for finding in findings {
        *findings_by_game.entry(finding.game_id.clone()).or_insert(0) += 1;
    }

    for game in game_results {
        let findings_count = findings_by_game.get(&game.game_id).copied().unwrap_or(0);
        game.findings_count = findings_count;

        if game.status == "missing" && game.detected_files > 0 {
            game.status = "no-history".to_string();
            game.note = "Local artifacts were found from configured paths, but no history token was extracted."
                .to_string();
        }

        if game.status != "ready" && findings_count > 0 {
            game.status = "ready".to_string();
            if game.completeness == "none" {
                game.completeness = "partial".to_string();
            }
            game.note =
                "Extractable history token(s) were found from local artifacts or manual fallback inputs."
                    .to_string();
        }
    }
}

fn apply_history_pulls_to_game_results(
    game_results: &mut [GameScanResult],
    history_pulls: &[HistoryPullRow],
) {
    let mut pulls_by_game = BTreeMap::<String, usize>::new();
    for row in history_pulls {
        *pulls_by_game.entry(row.game_id.clone()).or_insert(0) += 1;
    }

    for game in game_results {
        let total = pulls_by_game.get(&game.game_id).copied().unwrap_or(0);
        if total == 0 {
            continue;
        }
        game.status = "ready".to_string();
        game.completeness = "full".to_string();
        game.findings_count = total;
        game.note = format!("Loaded {total} pull-history rows from official API.");
    }
}

fn wuwa_banner_name(card_pool_type: i64) -> &'static str {
    match card_pool_type {
        1 => "Character Event",
        2 => "Weapon Event",
        3 => "Character Standard",
        4 => "Weapon Standard",
        5 => "Novice",
        6 => "Beginner Choice",
        7 => "Beginner Choice (Gratitude)",
        _ => "Wuthering Waves",
    }
}

fn hoyo_banner_name(game_id: &str, gacha_type: &str) -> String {
    if game_id == "honkai-star-rail" {
        match gacha_type {
            "1" => "Stellar Warp",
            "2" => "Departure Warp",
            "11" => "Character Event Warp",
            "12" => "Light Cone Event Warp",
            _ => "Warp",
        }
        .to_string()
    } else {
        match gacha_type {
            "1" => "Stable Channel",
            "2" => "Exclusive Channel",
            "3" => "W-Engine Channel",
            "5" => "Bangboo Channel",
            "2001" => "Exclusive Channel",
            "2002" => "W-Engine Channel",
            "3001" => "Stable Channel",
            "5001" => "Bangboo Channel",
            _ => "Signal Search",
        }
        .to_string()
    }
}

fn query_map_from_token_url(token_url: &str) -> Option<BTreeMap<String, String>> {
    let parsed = Url::parse(token_url).ok()?;
    let mut out = BTreeMap::<String, String>::new();

    for (k, v) in parsed.query_pairs() {
        out.insert(k.to_string(), v.to_string());
    }
    if out.is_empty() {
        if let Some(fragment) = parsed.fragment() {
            if let Some(idx) = fragment.find('?') {
                let query = &fragment[idx + 1..];
                for (k, v) in url::form_urlencoded::parse(query.as_bytes()) {
                    out.insert(k.to_string(), v.to_string());
                }
            }
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

fn history_timeout_client() -> Result<Client, String> {
    Client::builder()
        .connect_timeout(std::time::Duration::from_secs(8))
        .timeout(std::time::Duration::from_secs(25))
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
        )
        .build()
        .map_err(|e| format!("Failed creating HTTP client: {e}"))
}

fn fetch_wuwa_history_from_token(
    token_url: &str,
    policy: &NetworkPolicy,
    client: &Client,
) -> Result<Vec<HistoryPullRow>, String> {
    let query = query_map_from_token_url(token_url)
        .ok_or_else(|| "WuWa token URL is missing required query params.".to_string())?;

    let server_id = query
        .get("svr_id")
        .cloned()
        .ok_or_else(|| "WuWa token missing svr_id.".to_string())?;
    let player_id = query
        .get("player_id")
        .cloned()
        .ok_or_else(|| "WuWa token missing player_id.".to_string())?;
    let language_code = query
        .get("lang")
        .cloned()
        .unwrap_or_else(|| "en".to_string());
    let record_id = query
        .get("record_id")
        .cloned()
        .ok_or_else(|| "WuWa token missing record_id.".to_string())?;
    let card_pool_id = query
        .get("resources_id")
        .cloned()
        .ok_or_else(|| "WuWa token missing resources_id.".to_string())?;
    let server_area = query
        .get("svr_area")
        .cloned()
        .unwrap_or_default()
        .to_ascii_lowercase();

    let host = Url::parse(token_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_default();
    let api_base = if server_area == "global" || host.contains(".aki-game.net") {
        "https://gmserver-api.aki-game2.net"
    } else {
        "https://gmserver-api.aki-game2.com"
    };
    let api_url = format!("{api_base}/gacha/record/query");
    policy.assert_url_allowed(&api_url)?;

    let mut rows = Vec::<HistoryPullRow>::new();
    let mut pool_errors = Vec::<String>::new();
    for card_pool_type in 1..=7 {
        let payload = serde_json::json!({
            "serverId": server_id,
            "playerId": player_id,
            "languageCode": language_code,
            "recordId": record_id,
            "cardPoolId": card_pool_id,
            "cardPoolType": card_pool_type,
        });

        let response = client
            .post(&api_url)
            .json(&payload)
            .send()
            .map_err(|e| format!("WuWa history request failed: {e}"))?;
        let response_json: Value = response
            .json()
            .map_err(|e| format!("WuWa history response parse failed: {e}"))?;
        let code = response_json
            .get("code")
            .or_else(|| response_json.get("Code"))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if code != 0 {
            let message = response_json
                .get("message")
                .or_else(|| response_json.get("Message"))
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            pool_errors.push(format!(
                "cardPoolType={card_pool_type} returned code={code} message={message}"
            ));
            continue;
        }

        let list = if let Some(array) = response_json.as_array() {
            array.to_vec()
        } else if let Some(array) = response_json.get("data").and_then(Value::as_array) {
            array.to_vec()
        } else if let Some(array) = response_json.get("Data").and_then(Value::as_array) {
            array.to_vec()
        } else if let Some(array) = response_json
            .get("data")
            .and_then(|v| v.get("list"))
            .and_then(Value::as_array)
        {
            array.to_vec()
        } else if let Some(array) = response_json
            .get("Data")
            .and_then(|v| v.get("List"))
            .and_then(Value::as_array)
        {
            array.to_vec()
        } else if let Some(array) = response_json.get("result").and_then(Value::as_array) {
            array.to_vec()
        } else if let Some(array) = response_json.get("Result").and_then(Value::as_array) {
            array.to_vec()
        } else {
            find_wuwa_items_array(&response_json).unwrap_or_default()
        };

        for (entry_index, entry) in list.into_iter().enumerate() {
            let item_name = entry
                .get("name")
                .or_else(|| entry.get("Name"))
                .and_then(Value::as_str)
                .unwrap_or("Unknown")
                .to_string();
            let item_type_name = entry
                .get("resourceType")
                .or_else(|| entry.get("ResourceType"))
                .or_else(|| entry.get("itemType"))
                .or_else(|| entry.get("ItemType"))
                .and_then(Value::as_str)
                .unwrap_or("Unknown")
                .to_string();
            let rarity = entry
                .get("qualityLevel")
                .or_else(|| entry.get("QualityLevel"))
                .or_else(|| entry.get("rankType"))
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let pulled_at = entry
                .get("time")
                .or_else(|| entry.get("Time"))
                .or_else(|| entry.get("createTime"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let record_hint = entry
                .get("id")
                .or_else(|| entry.get("Id"))
                .or_else(|| entry.get("recordId"))
                .or_else(|| entry.get("RecordId"))
                .or_else(|| entry.get("gachaId"))
                .or_else(|| entry.get("GachaId"))
                .map(Value::to_string)
                .unwrap_or_default();
            let pull_id =
                format!("ww:{card_pool_type}:{pulled_at}:{item_name}:{entry_index}:{record_hint}");

            rows.push(HistoryPullRow {
                game_id: "wuthering-waves".to_string(),
                banner_id: format!("wuthering-waves:{card_pool_type}"),
                banner_name: wuwa_banner_name(card_pool_type).to_string(),
                item_name,
                item_type_name,
                rarity,
                pulled_at,
                pull_id,
                source_url: sanitize_url(token_url),
            });
        }
    }

    if rows.is_empty() && !pool_errors.is_empty() {
        return Err(format!(
            "WuWa API returned no history rows. {}",
            pool_errors.join(" | ")
        ));
    }
    Ok(rows)
}

fn find_wuwa_items_array(value: &Value) -> Option<Vec<Value>> {
    if let Some(array) = value.as_array() {
        let looks_like_item_list = array.iter().any(|entry| {
            entry.get("name").is_some()
                || entry.get("Name").is_some()
                || entry.get("qualityLevel").is_some()
                || entry.get("QualityLevel").is_some()
                || entry.get("resourceType").is_some()
                || entry.get("ResourceType").is_some()
        });
        if looks_like_item_list {
            return Some(array.to_vec());
        }
    }

    if let Some(object) = value.as_object() {
        for nested in object.values() {
            if let Some(found) = find_wuwa_items_array(nested) {
                return Some(found);
            }
        }
    }
    None
}

fn fetch_hoyo_history_from_token(
    game_id: &str,
    token_url: &str,
    policy: &NetworkPolicy,
    client: &Client,
) -> Result<Vec<HistoryPullRow>, String> {
    policy.assert_url_allowed(token_url)?;
    let parsed = Url::parse(token_url).map_err(|e| format!("Invalid token URL: {e}"))?;
    let banner_types: Vec<&str> = if game_id == "honkai-star-rail" {
        vec!["1", "2", "11", "12"]
    } else {
        vec!["1", "2", "3", "5", "2001", "2002", "3001", "5001"]
    };

    let mut rows = Vec::<HistoryPullRow>::new();
    for banner_type in banner_types {
        let mut end_id = "0".to_string();
        for page in 1..=50 {
            let mut page_url = parsed.clone();
            {
                let mut query_pairs = page_url.query_pairs_mut();
                query_pairs.append_pair("gacha_type", banner_type);
                query_pairs.append_pair("page", &page.to_string());
                query_pairs.append_pair("size", "20");
                query_pairs.append_pair("end_id", &end_id);
            }
            let response = client
                .get(page_url.as_str())
                .send()
                .map_err(|e| format!("History request failed for {game_id}: {e}"))?;
            let payload: Value = response
                .json()
                .map_err(|e| format!("History JSON parse failed for {game_id}: {e}"))?;

            let retcode = payload
                .get("retcode")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            if retcode != 0 {
                break;
            }
            let Some(list) = payload
                .get("data")
                .and_then(|v| v.get("list"))
                .and_then(Value::as_array)
            else {
                break;
            };

            if list.is_empty() {
                break;
            }

            for entry in list {
                let gacha_type = entry
                    .get("gacha_type")
                    .and_then(Value::as_str)
                    .unwrap_or(banner_type);
                let item_name = entry
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("Unknown")
                    .to_string();
                let item_type_name = entry
                    .get("item_type")
                    .and_then(Value::as_str)
                    .unwrap_or("Unknown")
                    .to_string();
                let rarity = entry
                    .get("rank_type")
                    .and_then(Value::as_str)
                    .and_then(|rank| rank.parse::<i64>().ok())
                    .unwrap_or(0);
                let pulled_at = entry
                    .get("time")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let pull_id = entry
                    .get("id")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
                    .unwrap_or_else(|| format!("{item_name}:{pulled_at}:{gacha_type}"));

                rows.push(HistoryPullRow {
                    game_id: game_id.to_string(),
                    banner_id: format!("{game_id}:{gacha_type}"),
                    banner_name: hoyo_banner_name(game_id, gacha_type),
                    item_name,
                    item_type_name,
                    rarity,
                    pulled_at,
                    pull_id,
                    source_url: sanitize_url(token_url),
                });
            }

            if list.len() < 20 {
                break;
            }
            end_id = list
                .last()
                .and_then(|item| item.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("0")
                .to_string();
            if end_id == "0" {
                break;
            }
        }
    }
    Ok(rows)
}

fn dedup_history_pulls(rows: &mut Vec<HistoryPullRow>) {
    let mut seen = HashSet::<(String, String, String, String)>::new();
    rows.retain(|row| {
        seen.insert((
            row.game_id.clone(),
            row.banner_id.clone(),
            row.pull_id.clone(),
            row.item_name.clone(),
        ))
    });
}

fn fetch_history_pulls_from_tokens(
    findings: &[ScanFinding],
    policy: &NetworkPolicy,
) -> Result<Vec<HistoryPullRow>, String> {
    let client = history_timeout_client()?;
    let mut rows = Vec::<HistoryPullRow>::new();
    let mut errors = Vec::<String>::new();
    let mut attempts_by_game = BTreeMap::<String, usize>::new();

    let max_attempts_per_game = 3usize;
    for finding in findings.iter().rev() {
        if finding.kind != "url_token" {
            continue;
        }
        let attempts = attempts_by_game.entry(finding.game_id.clone()).or_insert(0);
        if *attempts >= max_attempts_per_game {
            continue;
        }
        let token_url = finding.raw_value.as_deref().unwrap_or(&finding.value);
        if finding.game_id == "wuthering-waves" && token_url.contains("/aki/gacha/index.html#/record")
        {
            *attempts += 1;
            match fetch_wuwa_history_from_token(token_url, policy, &client) {
                Ok(fetched) => {
                    rows.extend(fetched);
                    *attempts = max_attempts_per_game;
                }
                Err(err) => errors.push(format!("wuthering-waves: {err}")),
            }
            continue;
        }
        if (finding.game_id == "honkai-star-rail"
            || finding.game_id == "zenless-zone-zero"
            || finding.game_id == "endfield")
            && (token_url.contains("getGachaLog") || token_url.contains("getLdGachaLog"))
        {
            *attempts += 1;
            match fetch_hoyo_history_from_token(&finding.game_id, token_url, policy, &client) {
                Ok(fetched) => {
                    rows.extend(fetched);
                    *attempts = max_attempts_per_game;
                }
                Err(err) => errors.push(format!("{}: {err}", finding.game_id)),
            }
        }
    }

    if rows.is_empty() && !errors.is_empty() {
        errors.sort();
        errors.dedup();
        let preview = errors.iter().take(4).cloned().collect::<Vec<_>>();
        let remainder = errors.len().saturating_sub(preview.len());
        let suffix = if remainder > 0 {
            format!(" | ... ({remainder} more)")
        } else {
            String::new()
        };
        return Err(format!("{}{}", preview.join(" | "), suffix));
    }
    dedup_history_pulls(&mut rows);
    Ok(rows)
}

fn build_next_steps(game: &GameScanResult, draft: Option<&TroubleshootingDraft>) -> Vec<String> {
    let mut steps = Vec::<String>::new();

    match game.status.as_str() {
        "missing" => {
            steps.push(
                "Verify installation path and set a game-specific override if the game is outside the root path."
                    .to_string(),
            );
            steps.push(
                "Launch the game once to regenerate Player.log / Client.log before scanning again."
                    .to_string(),
            );
        }
        "no-history" => {
            steps.push(
                "Open in-game gacha history, then rerun scan to refresh local artifacts and URL tokens."
                    .to_string(),
            );
        }
        _ => {
            steps.push(
                "Re-run scans after new pulls to keep local history and diagnostics up to date."
                    .to_string(),
            );
        }
    }

    if game.completeness != "full" {
        steps.push(
            "Completeness is partial/none because local artifacts may only expose tokenized URL data."
                .to_string(),
        );
    }

    if let Some(value) = draft {
        match value.manual_fallback_status.as_str() {
            "missing" => steps.push(
                "Configured manual fallback path was not found. Update it to an existing file."
                    .to_string(),
            ),
            "read-error" => steps.push(
                "Configured manual fallback path could not be read. Check file permissions and retry."
                    .to_string(),
            ),
            "not-configured" => steps.push(
                "Optional: configure manual fallback with Player.log / Player-prev.log / data_2 for recovery scans."
                    .to_string(),
            ),
            _ => {}
        }
    }

    dedup_strings(&mut steps);
    steps
}

fn build_game_troubleshooting(
    game_results: &[GameScanResult],
    drafts: &[TroubleshootingDraft],
) -> Vec<GameTroubleshooting> {
    let mut draft_by_game = BTreeMap::<String, &TroubleshootingDraft>::new();
    for draft in drafts {
        draft_by_game.insert(draft.game_id.clone(), draft);
    }

    let mut rows = game_results
        .iter()
        .map(|game| {
            let draft = draft_by_game.get(&game.game_id).copied();
            let mut checked_path_hints = draft
                .map(|entry| entry.checked_path_hints.clone())
                .unwrap_or_default();
            let mut detected_source_files = draft
                .map(|entry| entry.detected_source_files.clone())
                .unwrap_or_default();
            dedup_strings(&mut checked_path_hints);
            dedup_strings(&mut detected_source_files);

            GameTroubleshooting {
                game_id: game.game_id.clone(),
                effective_root_path: draft
                    .map(|entry| entry.effective_root_path.clone())
                    .unwrap_or_default(),
                root_override_applied: draft
                    .map(|entry| entry.root_override_applied)
                    .unwrap_or(false),
                configured_manual_fallback_path: draft
                    .and_then(|entry| entry.configured_manual_fallback_path.clone()),
                manual_fallback_status: draft
                    .map(|entry| entry.manual_fallback_status.clone())
                    .unwrap_or_else(|| "not-configured".to_string()),
                checked_path_hints,
                detected_source_files,
                status: game.status.clone(),
                completeness: game.completeness.clone(),
                note: game.note.clone(),
                next_steps: build_next_steps(game, draft),
            }
        })
        .collect::<Vec<_>>();

    rows.sort_by(|a, b| a.game_id.cmp(&b.game_id));
    rows
}

fn normalize_path_key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase()
}

fn dedup_paths(paths: &mut Vec<PathBuf>) {
    let mut seen = HashSet::<String>::new();
    paths.retain(|path| seen.insert(normalize_path_key(path)));
}

fn collect_existing_files(candidates: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out = Vec::<PathBuf>::new();
    for candidate in candidates {
        if candidate.exists() && candidate.is_file() {
            out.push(candidate);
        }
    }
    dedup_paths(&mut out);
    out
}

fn build_log_candidates(root: &Path, game_roots: &[&str], file_names: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::<PathBuf>::new();
    for game_root in game_roots {
        for file_name in file_names {
            out.push(root.join(game_root).join(file_name));
        }
    }
    out
}

fn wuwa_install_roots(root: &Path) -> Vec<PathBuf> {
    let candidates = vec![
        root.join("WutheringWaves"),
        root.join("Wuthering Waves"),
        root.join("WutheringWaves Game"),
        root.join("Wuthering Waves Game"),
        root.join("Wuthering Waves").join("Wuthering Waves Game"),
        root.join("Games").join("Wuthering Waves Game"),
        root.join("Games")
            .join("Wuthering Waves")
            .join("Wuthering Waves Game"),
        root.join("Program Files")
            .join("Wuthering Waves")
            .join("Wuthering Waves Game"),
        root.join("Program Files (x86)")
            .join("Wuthering Waves")
            .join("Wuthering Waves Game"),
        root.join("SteamLibrary")
            .join("steamapps")
            .join("common")
            .join("Wuthering Waves"),
        root.join("SteamLibrary")
            .join("steamapps")
            .join("common")
            .join("Wuthering Waves")
            .join("Wuthering Waves Game"),
        root.join("Steam")
            .join("steamapps")
            .join("common")
            .join("Wuthering Waves"),
        root.join("Steam")
            .join("steamapps")
            .join("common")
            .join("Wuthering Waves")
            .join("Wuthering Waves Game"),
        root.join("Program Files")
            .join("Steam")
            .join("steamapps")
            .join("common")
            .join("Wuthering Waves"),
        root.join("Program Files")
            .join("Steam")
            .join("steamapps")
            .join("common")
            .join("Wuthering Waves")
            .join("Wuthering Waves Game"),
        root.join("Program Files (x86)")
            .join("Steam")
            .join("steamapps")
            .join("common")
            .join("Wuthering Waves"),
        root.join("Program Files (x86)")
            .join("Steam")
            .join("steamapps")
            .join("common")
            .join("Wuthering Waves")
            .join("Wuthering Waves Game"),
        root.join("Games")
            .join("Steam")
            .join("steamapps")
            .join("common")
            .join("Wuthering Waves"),
        root.join("Games")
            .join("Steam")
            .join("steamapps")
            .join("common")
            .join("Wuthering Waves")
            .join("Wuthering Waves Game"),
        root.join("Program Files")
            .join("Epic Games")
            .join("WutheringWavesj3oFh"),
        root.join("Program Files")
            .join("Epic Games")
            .join("WutheringWavesj3oFh")
            .join("Wuthering Waves Game"),
        root.join("Program Files (x86)")
            .join("Epic Games")
            .join("WutheringWavesj3oFh"),
        root.join("Program Files (x86)")
            .join("Epic Games")
            .join("WutheringWavesj3oFh")
            .join("Wuthering Waves Game"),
    ];
    let mut out = candidates;
    dedup_paths(&mut out);
    out
}

fn build_wuwa_log_candidates(root: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::<PathBuf>::new();
    let install_roots = wuwa_install_roots(root);
    let log_suffixes = [
        PathBuf::from("Client.log"),
        PathBuf::from("debug.log"),
        PathBuf::from("Client").join("Saved").join("Logs").join("Client.log"),
        PathBuf::from("Client")
            .join("Saved")
            .join("Logs")
            .join("Client-Win64-Shipping.log"),
        PathBuf::from("Client").join("Saved").join("Logs").join("debug.log"),
        PathBuf::from("Client")
            .join("Binaries")
            .join("Win64")
            .join("ThirdParty")
            .join("KrPcSdk_Global")
            .join("KRSDKRes")
            .join("KRSDKWebView")
            .join("debug.log"),
    ];

    for install_root in install_roots {
        for suffix in &log_suffixes {
            candidates.push(install_root.join(suffix));
        }
    }
    candidates.push(root.join("Client.log"));
    candidates.push(root.join("debug.log"));
    dedup_paths(&mut candidates);
    candidates
}

fn dedup_findings(findings: &mut Vec<ScanFinding>) {
    let mut seen = HashSet::<(String, String, String, String)>::new();
    findings.retain(|finding| {
        let dedup_value = finding
            .raw_value
            .as_deref()
            .unwrap_or(&finding.value)
            .to_string();
        seen.insert((
            finding.game_id.clone(),
            finding.source_file.clone(),
            finding.kind.clone(),
            dedup_value,
        ))
    });
}

fn scan_wuwa(root: &Path) -> Result<(Vec<ScanFinding>, GameScanResult), String> {
    let game_id = "wuthering-waves".to_string();
    let mut findings = Vec::<ScanFinding>::new();

    let mut log_candidates = build_wuwa_log_candidates(root);
    if let Some(local_low) = local_low_path() {
        for vendor in ["KuroGame", "Kuro Games"] {
            for root_name in ["WutheringWaves", "Wuthering Waves"] {
                log_candidates.push(
                    local_low
                        .join(vendor)
                        .join(root_name)
                        .join("Client")
                        .join("Saved")
                        .join("Logs")
                        .join("Client.log"),
                );
                log_candidates.push(
                    local_low
                        .join(vendor)
                        .join(root_name)
                        .join("Client")
                        .join("Saved")
                        .join("Logs")
                        .join("debug.log"),
                );
            }
        }
    }
    let mut files = collect_existing_files(log_candidates);
    files.sort_by_key(|path| {
        fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|dur| dur.as_secs())
            .unwrap_or(0)
    });

    let pattern = Regex::new(r#"https?://[^\s"']*aki/gacha/index\.html#/record[^\s"']*"#)
        .map_err(|e| format!("Regex build failed for WuWa parser: {e}"))?;

    for log_file in &files {
        let text = read_text(log_file)?;
        if let Some(hit) = pattern.find_iter(&text).last() {
            let normalized = normalize_logged_url(hit.as_str());
            findings.push(ScanFinding {
                game_id: game_id.clone(),
                source_file: log_file.display().to_string(),
                kind: "url_token".to_string(),
                value: sanitize_url(&normalized),
                raw_value: Some(normalized),
            });
        }
    }
    dedup_findings(&mut findings);

    let result = summarize_game(
        &game_id,
        files.len(),
        findings.len(),
        if files.is_empty() {
            "No WuWa local logs detected from known install locations."
        } else if findings.is_empty() {
            "WuWa logs detected, but no convene history token was found."
        } else {
            "Convene token(s) found in local WuWa logs."
        },
    );

    Ok((findings, result))
}

fn scan_hsr(root: &Path) -> Result<(Vec<ScanFinding>, GameScanResult), String> {
    let game_id = "honkai-star-rail".to_string();
    let mut files = Vec::<PathBuf>::new();
    let mut findings = Vec::<ScanFinding>::new();

    let mut player_log_candidates = build_log_candidates(
        root,
        &HSR_ROOT_DIR_HINTS,
        &["Player.log", "Player-prev.log"],
    );
    player_log_candidates.push(root.join("Player.log"));
    player_log_candidates.push(root.join("Player-prev.log"));
    if let Some(local_low) = local_low_path() {
        for vendor in ["Cognosphere", "miHoYo", "HoYoverse"] {
            player_log_candidates.push(local_low.join(vendor).join("Star Rail").join("Player.log"));
            player_log_candidates.push(
                local_low
                    .join(vendor)
                    .join("Star Rail")
                    .join("Player-prev.log"),
            );
        }
    }
    let player_logs = collect_existing_files(player_log_candidates);

    let mut inferred_paths: Vec<PathBuf> = Vec::new();
    for log in &player_logs {
        files.push(log.clone());
        let text = read_text(log)?;
        findings.extend(extract_gacha_log_findings(&text, &game_id, log)?);
        if let Some(path) = extract_hsr_game_path(&text) {
            inferred_paths.push(path);
        }
    }
    dedup_paths(&mut inferred_paths);

    let mut data2_candidates = Vec::<PathBuf>::new();
    if inferred_paths.is_empty() {
        for root_hint in HSR_ROOT_DIR_HINTS {
            data2_candidates.extend(find_data2_files(&root.join(root_hint)));
        }
    } else {
        for game_path in &inferred_paths {
            data2_candidates.extend(guess_data2_from_game_path(game_path));
        }
    }

    for data2 in collect_existing_files(data2_candidates) {
        files.push(data2.clone());
        let text = read_text(&data2)?;
        findings.extend(extract_gacha_log_findings(&text, &game_id, &data2)?);
    }

    dedup_paths(&mut files);
    dedup_findings(&mut findings);

    let result = summarize_game(
        &game_id,
        files.len(),
        findings.len(),
        if files.is_empty() {
            "No HSR player logs or cache files detected under root path."
        } else if findings.is_empty() {
            "HSR local artifacts were detected, but no warp history token was found."
        } else {
            "Warp token(s) found in local HSR artifacts."
        },
    );

    Ok((findings, result))
}

fn scan_zzz(root: &Path) -> Result<(Vec<ScanFinding>, GameScanResult), String> {
    let game_id = "zenless-zone-zero".to_string();
    let mut files = Vec::<PathBuf>::new();
    let mut findings = Vec::<ScanFinding>::new();

    let mut player_log_candidates = build_log_candidates(
        root,
        &ZZZ_ROOT_DIR_HINTS,
        &["Player.log", "Player-prev.log"],
    );
    player_log_candidates.push(root.join("Player.log"));
    player_log_candidates.push(root.join("Player-prev.log"));
    if let Some(local_low) = local_low_path() {
        for vendor in ["miHoYo", "HoYoverse", "Cognosphere"] {
            for game_root in ["ZenlessZoneZero", "Zenless Zone Zero"] {
                player_log_candidates
                    .push(local_low.join(vendor).join(game_root).join("Player.log"));
                player_log_candidates.push(
                    local_low
                        .join(vendor)
                        .join(game_root)
                        .join("Player-prev.log"),
                );
            }
        }
    }
    let player_logs = collect_existing_files(player_log_candidates);

    let zzz_gacha_url_pattern =
        Regex::new(r#"https://gs\.hoyoverse\.com/nap/event/[^\s"']*gacha[^\s"']*"#)
            .map_err(|e| format!("Regex build failed for ZZZ parser: {e}"))?;

    let mut inferred_paths: Vec<PathBuf> = Vec::new();
    for log in &player_logs {
        files.push(log.clone());
        let text = read_text(log)?;

        for hit in zzz_gacha_url_pattern.find_iter(&text) {
            let normalized = normalize_logged_url(hit.as_str());
            findings.push(ScanFinding {
                game_id: game_id.clone(),
                source_file: log.display().to_string(),
                kind: "url_token".to_string(),
                value: sanitize_url(&normalized),
                raw_value: Some(normalized),
            });
        }
        findings.extend(extract_gacha_log_findings(&text, &game_id, log)?);

        if let Some(path) = extract_subsystems_game_path(&text) {
            inferred_paths.push(path);
        }
    }
    dedup_paths(&mut inferred_paths);

    let mut data2_candidates = Vec::<PathBuf>::new();
    if inferred_paths.is_empty() {
        for root_hint in ZZZ_ROOT_DIR_HINTS {
            data2_candidates.extend(find_data2_files(&root.join(root_hint)));
        }
    } else {
        for game_path in &inferred_paths {
            data2_candidates.extend(guess_data2_from_game_path(game_path));
        }
    }

    for data2 in collect_existing_files(data2_candidates) {
        files.push(data2.clone());
        let text = read_text(&data2)?;
        findings.extend(extract_gacha_log_findings(&text, &game_id, &data2)?);
    }

    dedup_paths(&mut files);
    dedup_findings(&mut findings);

    let result = summarize_game(
        &game_id,
        files.len(),
        findings.len(),
        if files.is_empty() {
            "No ZZZ player logs or cache files detected under root path."
        } else if findings.is_empty() {
            "ZZZ local artifacts were detected, but no signal history token was found."
        } else {
            "Signal token(s) found in local ZZZ artifacts."
        },
    );

    Ok((findings, result))
}

fn scan_endfield(root: &Path) -> Result<(Vec<ScanFinding>, GameScanResult), String> {
    let game_id = "endfield".to_string();
    let mut files = Vec::<PathBuf>::new();
    let mut findings = Vec::<ScanFinding>::new();

    let mut player_log_candidates = build_log_candidates(
        root,
        &ENDFIELD_ROOT_DIR_HINTS,
        &["Player.log", "Player-prev.log"],
    );
    player_log_candidates.push(root.join("Player.log"));
    player_log_candidates.push(root.join("Player-prev.log"));
    if let Some(local_low) = local_low_path() {
        for vendor in ["Hypergryph", "GRYPHLINE", "Endfield"] {
            for game_root in ["Endfield", "EndField"] {
                player_log_candidates
                    .push(local_low.join(vendor).join(game_root).join("Player.log"));
                player_log_candidates.push(
                    local_low
                        .join(vendor)
                        .join(game_root)
                        .join("Player-prev.log"),
                );
            }
        }
    }
    let player_logs = collect_existing_files(player_log_candidates);

    let generic_pattern = Regex::new(r#"https?://[^\s"']+"#)
        .map_err(|e| format!("Regex build failed for Endfield parser: {e}"))?;

    let mut inferred_paths: Vec<PathBuf> = Vec::new();
    for log in &player_logs {
        files.push(log.clone());
        let text = read_text(log)?;
        findings.extend(extract_gacha_log_findings(&text, &game_id, log)?);

        for hit in generic_pattern.find_iter(&text) {
            let candidate = normalize_logged_url(hit.as_str());
            let candidate_lower = candidate.to_ascii_lowercase();
            let looks_like_history_source = candidate_lower.contains("gacha")
                || candidate_lower.contains("wish")
                || candidate_lower.contains("pool")
                || candidate_lower.contains("authkey");
            if looks_like_history_source {
                let kind = if candidate_lower.contains("authkey=") {
                    "url_token"
                } else {
                    "possible_history_source"
                };
                findings.push(ScanFinding {
                    game_id: game_id.clone(),
                    source_file: log.display().to_string(),
                    kind: kind.to_string(),
                    value: sanitize_url(&candidate),
                    raw_value: Some(candidate),
                });
            }
        }

        if let Some(path) = extract_subsystems_game_path(&text) {
            inferred_paths.push(path);
        }
    }
    dedup_paths(&mut inferred_paths);

    let mut data2_candidates = Vec::<PathBuf>::new();
    if inferred_paths.is_empty() {
        for root_hint in ENDFIELD_ROOT_DIR_HINTS {
            data2_candidates.extend(find_data2_files(&root.join(root_hint)));
        }
    } else {
        for game_path in &inferred_paths {
            data2_candidates.extend(guess_data2_from_game_path(game_path));
        }
    }

    for data2 in collect_existing_files(data2_candidates) {
        files.push(data2.clone());
        let text = read_text(&data2)?;
        findings.extend(extract_gacha_log_findings(&text, &game_id, &data2)?);
    }

    dedup_paths(&mut files);
    dedup_findings(&mut findings);

    let result = summarize_endfield(&game_id, files.len(), &findings);
    Ok((findings, result))
}

fn summarize_game(
    game_id: &str,
    detected_files: usize,
    findings_count: usize,
    note: &str,
) -> GameScanResult {
    let (status, completeness) = if detected_files == 0 {
        ("missing", "none")
    } else if findings_count == 0 {
        ("no-history", "none")
    } else {
        ("ready", "partial")
    };

    GameScanResult {
        game_id: game_id.to_string(),
        status: status.to_string(),
        completeness: completeness.to_string(),
        detected_files,
        findings_count,
        note: note.to_string(),
    }
}

fn summarize_endfield(
    game_id: &str,
    detected_files: usize,
    findings: &[ScanFinding],
) -> GameScanResult {
    let has_url_token = findings.iter().any(|finding| finding.kind == "url_token");
    let has_possible_source = findings
        .iter()
        .any(|finding| finding.kind == "possible_history_source");
    let findings_count = findings.len();

    let (status, completeness, note) = if detected_files == 0 {
        (
            "missing",
            "none",
            "No Endfield player logs or cache files detected under root path.",
        )
    } else if has_url_token {
        (
            "ready",
            "partial",
            "Endfield local artifacts include history token candidate(s).",
        )
    } else if has_possible_source {
        (
            "no-history",
            "partial",
            "Endfield local artifacts include possible history sources, but no confirmed pull-history token was found.",
        )
    } else {
        (
            "no-history",
            "partial",
            "Endfield local artifacts were detected, but no clear pull-history token pattern was found.",
        )
    };

    GameScanResult {
        game_id: game_id.to_string(),
        status: status.to_string(),
        completeness: completeness.to_string(),
        detected_files,
        findings_count,
        note: note.to_string(),
    }
}

fn build_network_policy_state(
    policy: &NetworkPolicy,
    findings: &[ScanFinding],
) -> NetworkPolicyState {
    let mut approved_candidates = 0usize;
    let mut blocked_candidates = 0usize;

    for finding in findings {
        if let Some(host) = extract_url_host(&finding.value) {
            if !is_approved_official_api_host(&host) {
                continue;
            }
            if policy.assert_url_allowed(&finding.value).is_ok() {
                approved_candidates += 1;
            } else {
                blocked_candidates += 1;
            }
        }
    }

    let message = if policy.online_calls_allowed() {
        if approved_candidates > 0 {
            format!(
                "Online exception path is armed for {approved_candidates} officially confirmed API URL candidate(s)."
            )
        } else {
            "Online exception path is enabled, but no officially confirmed API URL candidates were detected."
                .to_string()
        }
    } else if policy.exception_requested() && !policy.confirmed_hosts_available() {
        "Exception mode was requested, but no officially confirmed API hosts are configured. Runtime stayed strict local-only."
            .to_string()
    } else if blocked_candidates > 0 {
        format!(
            "Strict local-only mode blocked {blocked_candidates} officially confirmed API URL candidate(s)."
        )
    } else {
        "Strict local-only mode blocked all online behavior during scan.".to_string()
    };

    NetworkPolicyState {
        mode: policy.mode().to_string(),
        online_calls_allowed: policy.online_calls_allowed(),
        approved_hosts: policy.approved_hosts(),
        approved_candidates,
        blocked_candidates,
        message,
    }
}

fn extract_hsr_game_path(text: &str) -> Option<PathBuf> {
    for line in text.lines().take(30) {
        if let Some(suffix) = line.strip_prefix("Loading player data from ") {
            let normalized = suffix.replace("data.unity3d", "");
            if !normalized.trim().is_empty() {
                return Some(PathBuf::from(normalized.trim()));
            }
        }
    }
    None
}

fn extract_subsystems_game_path(text: &str) -> Option<PathBuf> {
    for line in text.lines().take(40) {
        if let Some(suffix) = line.strip_prefix("[Subsystems] Discovering subsystems at path ") {
            let normalized = suffix.replace("UnitySubsystems", "");
            if !normalized.trim().is_empty() {
                return Some(PathBuf::from(normalized.trim()));
            }
        }
    }
    None
}

fn guess_data2_from_game_path(game_path: &Path) -> Vec<PathBuf> {
    let mut cache_roots = vec![game_path.to_path_buf()];
    if let Some(name) = game_path.file_name().and_then(|v| v.to_str()) {
        if name.ends_with("_Data") {
            if let Some(parent) = game_path.parent() {
                cache_roots.push(parent.to_path_buf());
            }
        }
    }
    if let Some(parent) = game_path.parent() {
        if let Some(name) = parent.file_name().and_then(|v| v.to_str()) {
            if name.ends_with("_Data") {
                if let Some(grand_parent) = parent.parent() {
                    cache_roots.push(grand_parent.to_path_buf());
                }
            }
        }
    }
    dedup_paths(&mut cache_roots);

    let version_re = match Regex::new(r#"^\d+\.\d+\.\d+\.\d+$"#) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let mut out = Vec::<PathBuf>::new();
    let mut versioned_candidates: Vec<(u64, PathBuf)> = Vec::new();
    for cache_root in cache_roots {
        let web_caches = cache_root.join("webCaches");
        out.push(web_caches.join("Cache").join("Cache_Data").join("data_2"));
        if !web_caches.exists() {
            continue;
        }

        if let Ok(entries) = fs::read_dir(&web_caches) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                if !version_re.is_match(&name) {
                    continue;
                }
                versioned_candidates.push((
                    version_score(&name),
                    path.join("Cache").join("Cache_Data").join("data_2"),
                ));
            }
        }
    }

    versioned_candidates.sort_by(|a, b| b.0.cmp(&a.0));
    out.extend(versioned_candidates.into_iter().map(|(_, p)| p));
    dedup_paths(&mut out);
    out
}

fn version_score(version: &str) -> u64 {
    let mut score = String::new();
    for part in version.split('.') {
        let mut p = part.trim().to_string();
        while p.len() < 3 {
            p.insert(0, '0');
        }
        score.push_str(&p);
    }
    score.parse::<u64>().unwrap_or_default()
}

fn extract_url_host(url: &str) -> Option<String> {
    let scheme_end = url.find("://")?;
    let rest = &url[scheme_end + 3..];
    let host_port = rest
        .split(|c| c == '/' || c == '?' || c == '#')
        .next()
        .unwrap_or("")
        .trim();
    if host_port.is_empty() {
        return None;
    }
    let host = host_port.split(':').next().unwrap_or("").trim();
    if host.is_empty() {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

fn is_approved_official_api_host(host: &str) -> bool {
    CONFIRMED_OFFICIAL_API_HOSTS
        .iter()
        .any(|allowed| host.eq_ignore_ascii_case(allowed))
}

fn extract_gacha_log_findings(
    text: &str,
    game_id: &str,
    source: &Path,
) -> Result<Vec<ScanFinding>, String> {
    let mut findings = Vec::<ScanFinding>::new();
    let pattern = Regex::new(r#"https?://[^\s"'\x00]*(?:getGachaLog|getLdGachaLog)[^\s"'\x00]*"#)
        .map_err(|e| format!("Regex build failed for cache parser: {e}"))?;

    for hit in pattern.find_iter(text) {
        let normalized = normalize_logged_url(hit.as_str());
        findings.push(ScanFinding {
            game_id: game_id.to_string(),
            source_file: source.display().to_string(),
            kind: "url_token".to_string(),
            value: sanitize_url(&normalized),
            raw_value: Some(normalized),
        });
    }
    Ok(findings)
}

fn find_data2_files(search_root: &Path) -> Vec<PathBuf> {
    if !search_root.exists() {
        return Vec::new();
    }

    let mut out = Vec::<PathBuf>::new();
    for entry in WalkDir::new(search_root)
        .max_depth(8)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if entry.file_type().is_file()
            && entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("data_2")
        {
            out.push(entry.path().to_path_buf());
        }
    }
    dedup_paths(&mut out);
    out
}

fn trim_trailing_delimiters(value: &str) -> &str {
    value.trim_end_matches(|c: char| c == ',' || c == ']' || c == ')' || c == '"')
}

fn normalize_logged_url(value: &str) -> String {
    trim_trailing_delimiters(value)
        .trim()
        .trim_matches('"')
        .replace("\\u0026", "&")
        .replace("\\u003d", "=")
        .replace("\\u003f", "?")
        .replace("\\u002F", "/")
        .replace("\\/", "/")
        .replace("&amp;", "&")
}

fn sanitize_url(url: &str) -> String {
    if let Some(start) = url.find("authkey=") {
        let value_start = start + "authkey=".len();
        let value_end = url[value_start..]
            .find('&')
            .map(|i| value_start + i)
            .unwrap_or(url.len());

        let mut out = String::with_capacity(url.len());
        out.push_str(&url[..value_start]);
        out.push_str("[REDACTED]");
        out.push_str(&url[value_end..]);
        return out;
    }
    url.to_string()
}

fn read_text(path: &Path) -> Result<String, String> {
    fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
        .map_err(|e| format!("Failed reading {}: {e}", path.display()))
}

fn local_low_path() -> Option<PathBuf> {
    std::env::var("APPDATA")
        .ok()
        .map(PathBuf::from)
        .and_then(|p| p.parent().map(|parent| parent.to_path_buf()))
        .map(|local| local.join("LocalLow"))
}

fn unix_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn database_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed resolving app data dir: {e}"))?;
    fs::create_dir_all(&app_data)
        .map_err(|e| format!("Failed creating app data dir {}: {e}", app_data.display()))?;
    Ok(app_data.join("tracker.db"))
}

fn apply_migrations(conn: &mut Connection) -> Result<(), String> {
    let tx = conn
        .transaction()
        .map_err(|e| format!("Failed starting DB migration transaction: {e}"))?;

    tx.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at INTEGER NOT NULL
        );
        ",
    )
    .map_err(|e| format!("Failed preparing migrations table: {e}"))?;

    let mut current_version: i64 = tx
        .query_row("PRAGMA user_version;", [], |row| row.get(0))
        .map_err(|e| format!("Failed reading schema version: {e}"))?;

    if current_version < 1 {
        tx.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS scans (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                scanned_at TEXT NOT NULL,
                root_path TEXT NOT NULL,
                strict_offline INTEGER NOT NULL,
                allow_official_api_exceptions INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS game_results (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                scan_id INTEGER NOT NULL,
                game_id TEXT NOT NULL,
                status TEXT NOT NULL,
                completeness TEXT NOT NULL,
                detected_files INTEGER NOT NULL,
                findings_count INTEGER NOT NULL,
                note TEXT NOT NULL,
                FOREIGN KEY(scan_id) REFERENCES scans(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS findings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                scan_id INTEGER NOT NULL,
                game_id TEXT NOT NULL,
                source_file TEXT NOT NULL,
                kind TEXT NOT NULL,
                value TEXT NOT NULL,
                FOREIGN KEY(scan_id) REFERENCES scans(id) ON DELETE CASCADE
            );
            ",
        )
        .map_err(|e| format!("Failed applying schema migration v1: {e}"))?;

        tx.execute(
            "INSERT OR REPLACE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
            params![1i64, unix_ts() as i64],
        )
        .map_err(|e| format!("Failed recording migration v1: {e}"))?;

        current_version = 1;
        tx.pragma_update(None, "user_version", current_version)
            .map_err(|e| format!("Failed updating schema version to v1: {e}"))?;
    }

    if current_version < 2 {
        tx.execute_batch(
            "
            DELETE FROM game_results
            WHERE scan_id IN (
                SELECT s.id
                FROM scans s
                JOIN (
                    SELECT scanned_at, root_path, strict_offline, allow_official_api_exceptions, MAX(id) AS keep_id
                    FROM scans
                    GROUP BY scanned_at, root_path, strict_offline, allow_official_api_exceptions
                ) kept
                    ON s.scanned_at = kept.scanned_at
                    AND s.root_path = kept.root_path
                    AND s.strict_offline = kept.strict_offline
                    AND s.allow_official_api_exceptions = kept.allow_official_api_exceptions
                WHERE s.id <> kept.keep_id
            );

            DELETE FROM findings
            WHERE scan_id IN (
                SELECT s.id
                FROM scans s
                JOIN (
                    SELECT scanned_at, root_path, strict_offline, allow_official_api_exceptions, MAX(id) AS keep_id
                    FROM scans
                    GROUP BY scanned_at, root_path, strict_offline, allow_official_api_exceptions
                ) kept
                    ON s.scanned_at = kept.scanned_at
                    AND s.root_path = kept.root_path
                    AND s.strict_offline = kept.strict_offline
                    AND s.allow_official_api_exceptions = kept.allow_official_api_exceptions
                WHERE s.id <> kept.keep_id
            );

            DELETE FROM scans
            WHERE id IN (
                SELECT s.id
                FROM scans s
                JOIN (
                    SELECT scanned_at, root_path, strict_offline, allow_official_api_exceptions, MAX(id) AS keep_id
                    FROM scans
                    GROUP BY scanned_at, root_path, strict_offline, allow_official_api_exceptions
                ) kept
                    ON s.scanned_at = kept.scanned_at
                    AND s.root_path = kept.root_path
                    AND s.strict_offline = kept.strict_offline
                    AND s.allow_official_api_exceptions = kept.allow_official_api_exceptions
                WHERE s.id <> kept.keep_id
            );

            DELETE FROM game_results
            WHERE id NOT IN (
                SELECT MAX(id)
                FROM game_results
                GROUP BY scan_id, game_id
            );

            DELETE FROM findings
            WHERE id NOT IN (
                SELECT MAX(id)
                FROM findings
                GROUP BY scan_id, game_id, source_file, kind, value
            );

            CREATE UNIQUE INDEX IF NOT EXISTS ux_scans_natural_key
            ON scans (scanned_at, root_path, strict_offline, allow_official_api_exceptions);

            CREATE UNIQUE INDEX IF NOT EXISTS ux_game_results_scan_game
            ON game_results (scan_id, game_id);

            CREATE UNIQUE INDEX IF NOT EXISTS ux_findings_scan_signature
            ON findings (scan_id, game_id, source_file, kind, value);

            CREATE INDEX IF NOT EXISTS idx_scans_recent
            ON scans (id DESC);

            CREATE INDEX IF NOT EXISTS idx_game_results_scan_id
            ON game_results (scan_id);

            CREATE INDEX IF NOT EXISTS idx_findings_scan_id
            ON findings (scan_id);

            CREATE INDEX IF NOT EXISTS idx_findings_scan_game
            ON findings (scan_id, game_id);
            ",
        )
        .map_err(|e| format!("Failed applying schema migration v2: {e}"))?;

        tx.execute(
            "INSERT OR REPLACE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
            params![2i64, unix_ts() as i64],
        )
        .map_err(|e| format!("Failed recording migration v2: {e}"))?;

        current_version = 2;
        tx.pragma_update(None, "user_version", current_version)
            .map_err(|e| format!("Failed updating schema version to v2: {e}"))?;
    }

    if current_version < 3 {
        tx.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS import_sessions (
                scan_id INTEGER PRIMARY KEY,
                scanned_at TEXT NOT NULL,
                root_path TEXT NOT NULL,
                strict_offline INTEGER NOT NULL,
                allow_official_api_exceptions INTEGER NOT NULL,
                total_pulls INTEGER NOT NULL DEFAULT 0,
                total_banners INTEGER NOT NULL DEFAULT 0,
                total_sources INTEGER NOT NULL DEFAULT 0,
                total_diagnostics INTEGER NOT NULL DEFAULT 0,
                FOREIGN KEY(scan_id) REFERENCES scans(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS import_pulls (
                id TEXT NOT NULL,
                scan_id INTEGER NOT NULL,
                game_id TEXT NOT NULL,
                banner_id TEXT NOT NULL,
                source_file TEXT NOT NULL,
                source_type TEXT NOT NULL,
                kind TEXT NOT NULL,
                value TEXT NOT NULL,
                item_name TEXT NULL,
                item_type_name TEXT NULL,
                rarity INTEGER NULL,
                pulled_at TEXT NULL,
                row_index INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY(scan_id, id),
                FOREIGN KEY(scan_id) REFERENCES scans(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS import_banners (
                id TEXT NOT NULL,
                scan_id INTEGER NOT NULL,
                game_id TEXT NOT NULL,
                name TEXT NOT NULL,
                pull_type TEXT NOT NULL,
                PRIMARY KEY(scan_id, id),
                FOREIGN KEY(scan_id) REFERENCES scans(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS import_sources (
                id TEXT NOT NULL,
                scan_id INTEGER NOT NULL,
                game_id TEXT NOT NULL,
                path TEXT NOT NULL,
                source_type TEXT NOT NULL,
                findings INTEGER NOT NULL,
                PRIMARY KEY(scan_id, id),
                FOREIGN KEY(scan_id) REFERENCES scans(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS import_diagnostics (
                id TEXT NOT NULL,
                scan_id INTEGER NOT NULL,
                severity TEXT NOT NULL,
                message TEXT NOT NULL,
                game_id TEXT NULL,
                PRIMARY KEY(scan_id, id),
                FOREIGN KEY(scan_id) REFERENCES scans(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_import_sessions_recent
            ON import_sessions (scan_id DESC);

            CREATE INDEX IF NOT EXISTS idx_import_pulls_scan_id
            ON import_pulls (scan_id);

            CREATE INDEX IF NOT EXISTS idx_import_banners_scan_id
            ON import_banners (scan_id);

            CREATE INDEX IF NOT EXISTS idx_import_sources_scan_id
            ON import_sources (scan_id);

            CREATE INDEX IF NOT EXISTS idx_import_diagnostics_scan_id
            ON import_diagnostics (scan_id);
            ",
        )
        .map_err(|e| format!("Failed applying schema migration v3: {e}"))?;

        tx.execute(
            "INSERT OR REPLACE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
            params![3i64, unix_ts() as i64],
        )
        .map_err(|e| format!("Failed recording migration v3: {e}"))?;

        current_version = 3;
        tx.pragma_update(None, "user_version", current_version)
            .map_err(|e| format!("Failed updating schema version to v3: {e}"))?;
    }

    if current_version < 4 {
        tx.execute_batch(
            "
            ALTER TABLE import_pulls ADD COLUMN item_name TEXT NULL;
            ALTER TABLE import_pulls ADD COLUMN item_type_name TEXT NULL;
            ALTER TABLE import_pulls ADD COLUMN rarity INTEGER NULL;
            ALTER TABLE import_pulls ADD COLUMN pulled_at TEXT NULL;
            ",
        )
        .or_else(|_| Ok(()))
        .map_err(|e: rusqlite::Error| format!("Failed applying schema migration v4: {e}"))?;

        tx.execute(
            "INSERT OR REPLACE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
            params![4i64, unix_ts() as i64],
        )
        .map_err(|e| format!("Failed recording migration v4: {e}"))?;

        current_version = 4;
        tx.pragma_update(None, "user_version", current_version)
            .map_err(|e| format!("Failed updating schema version to v4: {e}"))?;
    }

    if current_version < 5 {
        tx.execute_batch(
            "
            ALTER TABLE import_pulls ADD COLUMN row_index INTEGER NOT NULL DEFAULT 0;
            ",
        )
        .or_else(|_| Ok(()))
        .map_err(|e: rusqlite::Error| format!("Failed applying schema migration v5: {e}"))?;

        tx.execute(
            "INSERT OR REPLACE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
            params![5i64, unix_ts() as i64],
        )
        .map_err(|e| format!("Failed recording migration v5: {e}"))?;

        current_version = 5;
        tx.pragma_update(None, "user_version", current_version)
            .map_err(|e| format!("Failed updating schema version to v5: {e}"))?;
    }

    if current_version > DB_SCHEMA_VERSION {
        return Err(format!(
            "Database schema version {current_version} is newer than supported version {DB_SCHEMA_VERSION}."
        ));
    }

    tx.commit()
        .map_err(|e| format!("Failed committing DB migrations: {e}"))
}

fn open_db(path: &Path) -> Result<Connection, String> {
    let mut conn = Connection::open(path)
        .map_err(|e| format!("Failed opening database {}: {e}", path.display()))?;

    conn.pragma_update(None, "foreign_keys", 1)
        .map_err(|e| format!("Failed enabling foreign keys: {e}"))?;
    apply_migrations(&mut conn)?;

    Ok(conn)
}

fn persist_scan(
    conn: &mut Connection,
    scanned_at: &str,
    request: &ScanRequest,
    scan_root_path: &str,
    game_results: &[GameScanResult],
    findings: &[ScanFinding],
) -> Result<i64, String> {
    let tx = conn
        .transaction()
        .map_err(|e| format!("Failed starting DB transaction: {e}"))?;

    let scan_id = tx
        .query_row(
            "
            INSERT INTO scans (scanned_at, root_path, strict_offline, allow_official_api_exceptions)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(scanned_at, root_path, strict_offline, allow_official_api_exceptions)
            DO UPDATE SET scanned_at = excluded.scanned_at
            RETURNING id
            ",
            params![
                scanned_at,
                scan_root_path,
                if request.strict_offline { 1 } else { 0 },
                if request.allow_official_api_exceptions {
                    1
                } else {
                    0
                }
            ],
            |row| row.get(0),
        )
        .map_err(|e| format!("Failed writing scan row: {e}"))?;

    tx.execute(
        "DELETE FROM game_results WHERE scan_id = ?1",
        params![scan_id],
    )
    .map_err(|e| format!("Failed clearing existing game results: {e}"))?;
    tx.execute("DELETE FROM findings WHERE scan_id = ?1", params![scan_id])
        .map_err(|e| format!("Failed clearing existing findings: {e}"))?;

    let mut stable_game_results = game_results.to_vec();
    stable_game_results.sort_by(|a, b| a.game_id.cmp(&b.game_id));
    stable_game_results.dedup_by(|a, b| a.game_id == b.game_id);

    for result in &stable_game_results {
        tx.execute(
            "
            INSERT INTO game_results (scan_id, game_id, status, completeness, detected_files, findings_count, note)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(scan_id, game_id)
            DO UPDATE SET
                status = excluded.status,
                completeness = excluded.completeness,
                detected_files = excluded.detected_files,
                findings_count = excluded.findings_count,
                note = excluded.note
            ",
            params![
                scan_id,
                &result.game_id,
                &result.status,
                &result.completeness,
                result.detected_files as i64,
                result.findings_count as i64,
                &result.note
            ],
        )
        .map_err(|e| format!("Failed writing game result row: {e}"))?;
    }

    let mut stable_findings = findings.to_vec();
    stable_findings.sort_by(|a, b| {
        (
            a.game_id.as_str(),
            a.source_file.as_str(),
            a.kind.as_str(),
            a.value.as_str(),
        )
            .cmp(&(
                b.game_id.as_str(),
                b.source_file.as_str(),
                b.kind.as_str(),
                b.value.as_str(),
            ))
    });
    stable_findings.dedup_by(|a, b| {
        a.game_id == b.game_id
            && a.source_file == b.source_file
            && a.kind == b.kind
            && a.value == b.value
    });

    for finding in &stable_findings {
        tx.execute(
            "
            INSERT INTO findings (scan_id, game_id, source_file, kind, value)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(scan_id, game_id, source_file, kind, value)
            DO UPDATE SET value = excluded.value
            ",
            params![
                scan_id,
                &finding.game_id,
                &finding.source_file,
                &finding.kind,
                &finding.value
            ],
        )
        .map_err(|e| format!("Failed writing finding row: {e}"))?;
    }

    tx.commit()
        .map_err(|e| format!("Failed committing DB transaction: {e}"))?;
    Ok(scan_id)
}

fn persist_import_session(conn: &mut Connection, session: &ImportSession) -> Result<(), String> {
    let tx = conn
        .transaction()
        .map_err(|e| format!("Failed starting import transaction: {e}"))?;

    tx.execute(
        "
        INSERT INTO import_sessions (
            scan_id,
            scanned_at,
            root_path,
            strict_offline,
            allow_official_api_exceptions,
            total_pulls,
            total_banners,
            total_sources,
            total_diagnostics
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        ON CONFLICT(scan_id)
        DO UPDATE SET
            scanned_at = excluded.scanned_at,
            root_path = excluded.root_path,
            strict_offline = excluded.strict_offline,
            allow_official_api_exceptions = excluded.allow_official_api_exceptions,
            total_pulls = excluded.total_pulls,
            total_banners = excluded.total_banners,
            total_sources = excluded.total_sources,
            total_diagnostics = excluded.total_diagnostics
        ",
        params![
            session.id,
            &session.scanned_at,
            &session.root_path,
            if session.strict_offline { 1 } else { 0 },
            if session.allow_official_api_exceptions {
                1
            } else {
                0
            },
            session.pulls.len() as i64,
            session.banners.len() as i64,
            session.source_metadata.len() as i64,
            session.diagnostics.len() as i64
        ],
    )
    .map_err(|e| format!("Failed writing import session row: {e}"))?;

    tx.execute(
        "DELETE FROM import_pulls WHERE scan_id = ?1",
        params![session.id],
    )
    .map_err(|e| format!("Failed clearing existing import pulls: {e}"))?;
    tx.execute(
        "DELETE FROM import_banners WHERE scan_id = ?1",
        params![session.id],
    )
    .map_err(|e| format!("Failed clearing existing import banners: {e}"))?;
    tx.execute(
        "DELETE FROM import_sources WHERE scan_id = ?1",
        params![session.id],
    )
    .map_err(|e| format!("Failed clearing existing import sources: {e}"))?;
    tx.execute(
        "DELETE FROM import_diagnostics WHERE scan_id = ?1",
        params![session.id],
    )
    .map_err(|e| format!("Failed clearing existing import diagnostics: {e}"))?;

    let mut seen_pull_ids = HashSet::<String>::new();
    for (row_index, pull) in session.pulls.iter().enumerate() {
        if !seen_pull_ids.insert(pull.id.clone()) {
            continue;
        }
        tx.execute(
            "
            INSERT INTO import_pulls (id, scan_id, game_id, banner_id, source_file, source_type, kind, value, item_name, item_type_name, rarity, pulled_at, row_index)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            ON CONFLICT(scan_id, id)
            DO UPDATE SET
                game_id = excluded.game_id,
                banner_id = excluded.banner_id,
                source_file = excluded.source_file,
                source_type = excluded.source_type,
                kind = excluded.kind,
                value = excluded.value,
                item_name = excluded.item_name,
                item_type_name = excluded.item_type_name,
                rarity = excluded.rarity,
                pulled_at = excluded.pulled_at,
                row_index = excluded.row_index
            ",
            params![
                &pull.id,
                session.id,
                &pull.game_id,
                &pull.banner_id,
                &pull.source_file,
                &pull.source_type,
                &pull.kind,
                &pull.value,
                &pull.item_name,
                &pull.item_type_name,
                &pull.rarity,
                &pull.pulled_at,
                row_index as i64
            ],
        )
        .map_err(|e| format!("Failed writing import pull row: {e}"))?;
    }

    let mut stable_banners = session.banners.clone();
    stable_banners.sort_by(|a, b| a.id.cmp(&b.id));
    stable_banners.dedup_by(|a, b| a.id == b.id);
    for banner in &stable_banners {
        tx.execute(
            "
            INSERT INTO import_banners (id, scan_id, game_id, name, pull_type)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(scan_id, id)
            DO UPDATE SET
                game_id = excluded.game_id,
                name = excluded.name,
                pull_type = excluded.pull_type
            ",
            params![
                &banner.id,
                session.id,
                &banner.game_id,
                &banner.name,
                &banner.pull_type
            ],
        )
        .map_err(|e| format!("Failed writing import banner row: {e}"))?;
    }

    let mut stable_sources = session.source_metadata.clone();
    stable_sources.sort_by(|a, b| a.id.cmp(&b.id));
    stable_sources.dedup_by(|a, b| a.id == b.id);
    for source in &stable_sources {
        tx.execute(
            "
            INSERT INTO import_sources (id, scan_id, game_id, path, source_type, findings)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(scan_id, id)
            DO UPDATE SET
                game_id = excluded.game_id,
                path = excluded.path,
                source_type = excluded.source_type,
                findings = excluded.findings
            ",
            params![
                &source.id,
                session.id,
                &source.game_id,
                &source.path,
                &source.source_type,
                source.findings as i64
            ],
        )
        .map_err(|e| format!("Failed writing import source row: {e}"))?;
    }

    let mut stable_diagnostics = session.diagnostics.clone();
    stable_diagnostics.sort_by(|a, b| a.id.cmp(&b.id));
    stable_diagnostics.dedup_by(|a, b| a.id == b.id);
    for diagnostic in &stable_diagnostics {
        tx.execute(
            "
            INSERT INTO import_diagnostics (id, scan_id, severity, message, game_id)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(scan_id, id)
            DO UPDATE SET
                severity = excluded.severity,
                message = excluded.message,
                game_id = excluded.game_id
            ",
            params![
                &diagnostic.id,
                session.id,
                &diagnostic.severity,
                &diagnostic.message,
                &diagnostic.game_id
            ],
        )
        .map_err(|e| format!("Failed writing import diagnostic row: {e}"))?;
    }

    tx.commit()
        .map_err(|e| format!("Failed committing import transaction: {e}"))?;
    Ok(())
}

fn recompute_import_summary(conn: &Connection, scan_id: i64) -> Result<(), String> {
    conn.execute(
        "
        UPDATE import_sessions
        SET
            total_pulls = (SELECT COUNT(*) FROM import_pulls WHERE scan_id = ?1),
            total_banners = (SELECT COUNT(*) FROM import_banners WHERE scan_id = ?1),
            total_sources = (SELECT COUNT(*) FROM import_sources WHERE scan_id = ?1),
            total_diagnostics = (SELECT COUNT(*) FROM import_diagnostics WHERE scan_id = ?1)
        WHERE scan_id = ?1
        ",
        params![scan_id],
    )
    .map_err(|e| format!("Failed recomputing import summary: {e}"))?;
    Ok(())
}

fn load_import_session(conn: &Connection, scan_id: i64) -> Result<ImportSession, String> {
    let (scanned_at, root_path, strict_offline, allow_official_api_exceptions): (
        String,
        String,
        i64,
        i64,
    ) = conn
        .query_row(
            "
            SELECT scanned_at, root_path, strict_offline, allow_official_api_exceptions
            FROM import_sessions
            WHERE scan_id = ?1
            ",
            params![scan_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|e| format!("Failed loading import session {scan_id}: {e}"))?;

    let mut game_results_stmt = conn
        .prepare(
            "
            SELECT game_id, status, completeness, detected_files, findings_count, note
            FROM game_results
            WHERE scan_id = ?1
            ORDER BY game_id ASC
            ",
        )
        .map_err(|e| format!("Failed preparing game results query: {e}"))?;
    let game_results_iter = game_results_stmt
        .query_map(params![scan_id], |row| {
            Ok(GameScanResult {
                game_id: row.get(0)?,
                status: row.get(1)?,
                completeness: row.get(2)?,
                detected_files: row.get::<_, i64>(3)? as usize,
                findings_count: row.get::<_, i64>(4)? as usize,
                note: row.get(5)?,
            })
        })
        .map_err(|e| format!("Failed querying game results: {e}"))?;
    let mut game_results = Vec::<GameScanResult>::new();
    for row in game_results_iter {
        game_results.push(row.map_err(|e| format!("Failed reading game result row: {e}"))?);
    }

    let mut pulls_stmt = conn
        .prepare(
            "
            SELECT id, game_id, banner_id, source_file, source_type, kind, value, item_name, item_type_name, rarity, pulled_at, row_index
            FROM import_pulls
            WHERE scan_id = ?1
            ORDER BY row_index ASC, id ASC
            ",
        )
        .map_err(|e| format!("Failed preparing pulls query: {e}"))?;
    let pulls_iter = pulls_stmt
        .query_map(params![scan_id], |row| {
            Ok(Pull {
                id: row.get(0)?,
                game_id: row.get(1)?,
                banner_id: row.get(2)?,
                source_file: row.get(3)?,
                source_type: row.get(4)?,
                kind: row.get(5)?,
                value: row.get(6)?,
                item_name: row.get(7)?,
                item_type_name: row.get(8)?,
                rarity: row.get(9)?,
                pulled_at: row.get(10)?,
            })
        })
        .map_err(|e| format!("Failed querying import pulls: {e}"))?;
    let mut pulls = Vec::<Pull>::new();
    for row in pulls_iter {
        pulls.push(row.map_err(|e| format!("Failed reading import pull row: {e}"))?);
    }

    let mut banners_stmt = conn
        .prepare(
            "
            SELECT id, game_id, name, pull_type
            FROM import_banners
            WHERE scan_id = ?1
            ORDER BY id ASC
            ",
        )
        .map_err(|e| format!("Failed preparing banners query: {e}"))?;
    let banners_iter = banners_stmt
        .query_map(params![scan_id], |row| {
            Ok(Banner {
                id: row.get(0)?,
                game_id: row.get(1)?,
                name: row.get(2)?,
                pull_type: row.get(3)?,
            })
        })
        .map_err(|e| format!("Failed querying import banners: {e}"))?;
    let mut banners = Vec::<Banner>::new();
    for row in banners_iter {
        banners.push(row.map_err(|e| format!("Failed reading import banner row: {e}"))?);
    }

    let mut sources_stmt = conn
        .prepare(
            "
            SELECT id, game_id, path, source_type, findings
            FROM import_sources
            WHERE scan_id = ?1
            ORDER BY id ASC
            ",
        )
        .map_err(|e| format!("Failed preparing sources query: {e}"))?;
    let sources_iter = sources_stmt
        .query_map(params![scan_id], |row| {
            Ok(SourceMetadata {
                id: row.get(0)?,
                game_id: row.get(1)?,
                path: row.get(2)?,
                source_type: row.get(3)?,
                findings: row.get::<_, i64>(4)? as usize,
            })
        })
        .map_err(|e| format!("Failed querying import sources: {e}"))?;
    let mut source_metadata = Vec::<SourceMetadata>::new();
    for row in sources_iter {
        source_metadata.push(row.map_err(|e| format!("Failed reading import source row: {e}"))?);
    }

    let mut diagnostics_stmt = conn
        .prepare(
            "
            SELECT id, severity, message, game_id
            FROM import_diagnostics
            WHERE scan_id = ?1
            ORDER BY id ASC
            ",
        )
        .map_err(|e| format!("Failed preparing diagnostics query: {e}"))?;
    let diagnostics_iter = diagnostics_stmt
        .query_map(params![scan_id], |row| {
            Ok(ImportDiagnostic {
                id: row.get(0)?,
                severity: row.get(1)?,
                message: row.get(2)?,
                game_id: row.get(3)?,
            })
        })
        .map_err(|e| format!("Failed querying import diagnostics: {e}"))?;
    let mut diagnostics = Vec::<ImportDiagnostic>::new();
    for row in diagnostics_iter {
        diagnostics.push(row.map_err(|e| format!("Failed reading diagnostic row: {e}"))?);
    }

    let completeness_by_game = game_results
        .iter()
        .map(|game| (game.game_id.clone(), game.completeness.clone()))
        .collect::<BTreeMap<_, _>>();

    Ok(ImportSession {
        id: scan_id,
        scanned_at,
        root_path,
        strict_offline: strict_offline == 1,
        allow_official_api_exceptions: allow_official_api_exceptions == 1,
        game_results,
        completeness_by_game,
        pulls,
        banners,
        source_metadata,
        diagnostics,
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            run_local_scan,
            run_local_import,
            get_recent_scans,
            get_latest_import_session
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_scan_response(
        game_results: Vec<GameScanResult>,
        findings: Vec<ScanFinding>,
        notes: Vec<&str>,
    ) -> ScanResponse {
        ScanResponse {
            scan_id: 42,
            scanned_at: "1710000000".to_string(),
            root_path: r"C:\Samples".to_string(),
            strict_offline: true,
            allow_official_api_exceptions: false,
            network_policy: NetworkPolicyState {
                mode: "strict-local-only".to_string(),
                online_calls_allowed: false,
                approved_hosts: Vec::new(),
                approved_candidates: 0,
                blocked_candidates: 0,
                message: "Strict local-only mode blocked all online behavior during scan."
                    .to_string(),
            },
            game_results,
            findings,
            history_pulls: Vec::new(),
            notes: notes.into_iter().map(ToString::to_string).collect(),
            troubleshooting: Vec::new(),
        }
    }

    #[test]
    fn extract_gacha_log_findings_redacts_and_trims_urls() {
        let text = r#"
https://public-operation-hkrpg.mihoyo.com/common/gacha_record/api/getGachaLog?authkey=alpha123&lang=en-us],
https://public-operation-nap.hoyoverse.com/common/gacha_record/api/getLdGachaLog?foo=bar&authkey=beta456")
https://example.com/not-a-match?x=1
"#;
        let findings =
            extract_gacha_log_findings(text, "honkai-star-rail", Path::new(r"C:\Logs\Player.log"))
                .expect("sample parser input should not fail");

        assert_eq!(findings.len(), 2);
        assert!(findings
            .iter()
            .all(|finding| finding.kind == "url_token" && finding.game_id == "honkai-star-rail"));
        assert!(findings
            .iter()
            .all(|finding| finding.value.contains("authkey=[REDACTED]")));
        assert!(!findings.iter().any(|finding| {
            finding.value.contains("alpha123") || finding.value.contains("beta456")
        }));
        assert!(findings
            .iter()
            .all(|finding| !finding.value.ends_with(',') && !finding.value.ends_with(')')));
    }

    #[test]
    fn extract_game_paths_from_sample_player_logs() {
        let hsr_log = r#"
Initialize engine
Loading player data from C:\Games\StarRail\StarRail_Data\data.unity3d
remaining log lines
"#;
        let hsr_path = extract_hsr_game_path(hsr_log).expect("HSR sample path should be detected");
        let hsr_key = normalize_path_key(&hsr_path);
        assert!(hsr_key.contains("starrail_data"));
        assert!(!hsr_key.contains("data.unity3d"));

        let subsystem_log = r#"
Booting...
[Subsystems] Discovering subsystems at path C:\Games\ZenlessZoneZero\ZenlessZoneZero_Data\UnitySubsystems
Booted
"#;
        let subsystem_path = extract_subsystems_game_path(subsystem_log)
            .expect("subsystems sample path should be detected");
        let subsystem_key = normalize_path_key(&subsystem_path);
        assert!(subsystem_key.contains("zenlesszonezero_data"));
        assert!(!subsystem_key.contains("unitysubsystems"));
    }

    #[test]
    fn guess_data2_from_game_path_prefers_latest_versioned_cache() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock drift should not happen in test")
            .as_nanos();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-workspaces")
            .join(format!("guess-data2-{suffix}"));
        let game_path = root.join("ZenlessZoneZero");
        fs::create_dir_all(&game_path).expect("test workspace should be created");

        for version in ["1.0.0.1", "1.2.0.0", "1.10.0.0"] {
            fs::create_dir_all(
                game_path
                    .join("webCaches")
                    .join(version)
                    .join("Cache")
                    .join("Cache_Data"),
            )
            .expect("versioned cache folders should be created");
        }

        let guessed = guess_data2_from_game_path(&game_path);
        let normalized: Vec<String> = guessed
            .iter()
            .map(|path| normalize_path_key(path))
            .collect();

        assert!(normalized.len() >= 4);
        assert_eq!(
            normalized[0],
            normalize_path_key(
                &game_path
                    .join("webCaches")
                    .join("Cache")
                    .join("Cache_Data")
                    .join("data_2")
            )
        );
        assert_eq!(
            normalized[1],
            normalize_path_key(
                &game_path
                    .join("webCaches")
                    .join("1.10.0.0")
                    .join("Cache")
                    .join("Cache_Data")
                    .join("data_2")
            )
        );
        assert_eq!(
            normalized[2],
            normalize_path_key(
                &game_path
                    .join("webCaches")
                    .join("1.2.0.0")
                    .join("Cache")
                    .join("Cache_Data")
                    .join("data_2")
            )
        );
        assert_eq!(
            normalized[3],
            normalize_path_key(
                &game_path
                    .join("webCaches")
                    .join("1.0.0.1")
                    .join("Cache")
                    .join("Cache_Data")
                    .join("data_2")
            )
        );

        fs::remove_dir_all(root).expect("test workspace cleanup should succeed");
    }

    #[test]
    fn wuwa_log_candidates_include_launcher_nested_paths() {
        let root = PathBuf::from(r"D:\");
        let candidates = build_wuwa_log_candidates(&root);
        let normalized: Vec<String> = candidates
            .iter()
            .map(|path| normalize_path_key(path))
            .collect();

        let expected = normalize_path_key(
            &root
                .join("Games")
                .join("Wuthering Waves")
                .join("Wuthering Waves Game")
                .join("Client")
                .join("Saved")
                .join("Logs")
                .join("Client.log"),
        );
        assert!(
            normalized.iter().any(|path| path == &expected),
            "WuWa candidates should include nested launcher path pattern"
        );
    }

    #[test]
    fn build_import_session_aggregates_sources_and_banners() {
        let scan = sample_scan_response(
            vec![
                GameScanResult {
                    game_id: "honkai-star-rail".to_string(),
                    status: "ready".to_string(),
                    completeness: "partial".to_string(),
                    detected_files: 2,
                    findings_count: 1,
                    note: "Warp token(s) found.".to_string(),
                },
                GameScanResult {
                    game_id: "endfield".to_string(),
                    status: "no-history".to_string(),
                    completeness: "partial".to_string(),
                    detected_files: 1,
                    findings_count: 2,
                    note: "Possible source but no confirmed token.".to_string(),
                },
            ],
            vec![
                ScanFinding {
                    game_id: "honkai-star-rail".to_string(),
                    source_file: r"C:\Logs\Player.log".to_string(),
                    kind: "url_token".to_string(),
                    value: "https://example.com/getGachaLog?authkey=[REDACTED]".to_string(),
                    raw_value: None,
                },
                ScanFinding {
                    game_id: "endfield".to_string(),
                    source_file: r"C:\Caches\Cache\Cache_Data\data_2".to_string(),
                    kind: "possible_history_source".to_string(),
                    value: "https://example.com/gacha/source/1".to_string(),
                    raw_value: None,
                },
                ScanFinding {
                    game_id: "endfield".to_string(),
                    source_file: r"C:\Caches\Cache\Cache_Data\data_2".to_string(),
                    kind: "possible_history_source".to_string(),
                    value: "https://example.com/gacha/source/2".to_string(),
                    raw_value: None,
                },
            ],
            vec!["strict-local-only"],
        );

        let session = build_import_session(&scan);

        assert_eq!(session.pulls.len(), 3);
        assert!(session
            .banners
            .iter()
            .any(|banner| banner.id == "honkai-star-rail:history-url"));
        assert!(session
            .banners
            .iter()
            .any(|banner| banner.id == "endfield:possible-source"));
        assert!(session
            .banners
            .iter()
            .any(|banner| banner.id == "endfield:history-url"));

        let endfield_source = session
            .source_metadata
            .iter()
            .find(|source| source.id == r"endfield:C:\Caches\Cache\Cache_Data\data_2")
            .expect("endfield cache source metadata should be present");
        assert_eq!(endfield_source.findings, 2);
        assert_eq!(endfield_source.source_type, "cache");

        assert!(session.diagnostics.iter().any(
            |diagnostic| diagnostic.id == "status:endfield" && diagnostic.severity == "warning"
        ));
        assert!(!session
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.id == "summary:no-pulls"));
    }

    #[test]
    fn build_import_session_adds_no_pulls_warning_when_empty() {
        let scan = sample_scan_response(
            vec![GameScanResult {
                game_id: "zenless-zone-zero".to_string(),
                status: "no-history".to_string(),
                completeness: "none".to_string(),
                detected_files: 1,
                findings_count: 0,
                note: "Local artifact found without history token.".to_string(),
            }],
            Vec::new(),
            vec!["scan-note"],
        );

        let session = build_import_session(&scan);

        assert!(session.pulls.is_empty());
        assert!(session
            .banners
            .iter()
            .any(|banner| banner.id == "zenless-zone-zero:history-url"));
        assert!(session
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.id == "summary:no-pulls"
                && diagnostic.severity == "warning"));
    }
}
