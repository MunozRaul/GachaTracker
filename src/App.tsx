import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";
import {
  type ImportSession,
  type RunLocalImportResponse,
  type ScanResponse,
  type ScanSettings,
  type ScanSummary,
} from "./domain/models";

const GAME_SETTINGS = [
  {
    id: "wuthering-waves",
    label: "Wuthering Waves",
    fallbackHint: "Optional log/data_2 path if auto-detection misses files.",
  },
  {
    id: "honkai-star-rail",
    label: "Honkai: Star Rail",
    fallbackHint: "Point to Player.log, Player-prev.log, or data_2 cache file.",
  },
  {
    id: "zenless-zone-zero",
    label: "Zenless Zone Zero",
    fallbackHint: "Use Player.log or cache data_2 from the active session.",
  },
  {
    id: "endfield",
    label: "Endfield",
    fallbackHint: "Use Player.log / Player-prev.log when recruitment history was opened.",
  },
] as const;

const GAME_LABELS = Object.fromEntries(
  GAME_SETTINGS.map((game) => [game.id, game.label]),
) as Record<string, string>;

const defaultGameSettingState = () =>
  Object.fromEntries(GAME_SETTINGS.map((game) => [game.id, ""])) as Record<
    string,
    string
  >;

const sanitizeSettingsRecord = (record: Record<string, string>) =>
  Object.fromEntries(
    Object.entries(record)
      .map(([key, value]) => [key, value.trim()])
      .filter(([, value]) => value.length > 0),
  );

const manualFallbackStatusLabel: Record<string, string> = {
  "not-configured": "Not configured",
  loaded: "Loaded",
  missing: "Missing path",
  "read-error": "Read error",
};

function App() {
  const [rootPath, setRootPath] = useState("");
  const [autoDetectRoot, setAutoDetectRoot] = useState(true);
  const [strictOffline, setStrictOffline] = useState(true);
  const [allowOfficialApiExceptions, setAllowOfficialApiExceptions] =
    useState(false);
  const [gamePathOverrides, setGamePathOverrides] = useState(
    defaultGameSettingState,
  );
  const [manualFallbackPaths, setManualFallbackPaths] = useState(
    defaultGameSettingState,
  );
  const [isRunning, setIsRunning] = useState(false);
  const [error, setError] = useState("");
  const [scanResult, setScanResult] = useState<ScanResponse | null>(null);
  const [importSession, setImportSession] = useState<ImportSession | null>(
    null,
  );
  const [recentScans, setRecentScans] = useState<ScanSummary[]>([]);

  const scanSettings = useMemo<ScanSettings>(
    () => ({
      gamePaths: sanitizeSettingsRecord(gamePathOverrides),
      manualFallbackPaths: sanitizeSettingsRecord(manualFallbackPaths),
    }),
    [gamePathOverrides, manualFallbackPaths],
  );

  const totals = useMemo(() => {
    const games = importSession?.gameResults ?? [];
    return {
      games: games.length,
      ready: games.filter((g) => g.status === "ready").length,
      findings: importSession?.pulls.length ?? 0,
    };
  }, [importSession]);

  const completenessSummary = useMemo(() => {
    const games = scanResult?.gameResults ?? [];
    return {
      full: games.filter((game) => game.completeness === "full").length,
      partial: games.filter((game) => game.completeness === "partial").length,
      none: games.filter((game) => game.completeness === "none").length,
    };
  }, [scanResult]);

  const exceptionModeRequested = !strictOffline && allowOfficialApiExceptions;
  const diagnostics = importSession?.diagnostics ?? [];
  const troubleshooting = scanResult?.troubleshooting ?? [];

  const configuredPathOverrides = Object.keys(scanSettings.gamePaths).length;
  const configuredFallbacks = Object.keys(scanSettings.manualFallbackPaths).length;

  const latestScanLabel = useMemo(() => {
    if (!scanResult?.scannedAt) {
      return "No scan yet";
    }
    const parsed = new Date(scanResult.scannedAt);
    return Number.isNaN(parsed.getTime())
      ? scanResult.scannedAt
      : parsed.toLocaleString();
  }, [scanResult?.scannedAt]);

  const scanState = useMemo(() => {
    if (isRunning) {
      return { label: "Scan running", tone: "running" as const };
    }
    if (error) {
      return { label: "Scan failed", tone: "error" as const };
    }
    if (scanResult) {
      return { label: "Last scan complete", tone: "success" as const };
    }
    return { label: "Awaiting first scan", tone: "idle" as const };
  }, [error, isRunning, scanResult]);

  const formatTimestamp = (value: string) => {
    const parsed = new Date(value);
    return Number.isNaN(parsed.getTime()) ? value : parsed.toLocaleString();
  };

  async function loadRecentScans() {
    try {
      const rows = await invoke<ScanSummary[]>("get_recent_scans", { limit: 8 });
      setRecentScans(rows);
    } catch (e) {
      console.error(e);
    }
  }

  useEffect(() => {
    void loadRecentScans();
  }, []);

  async function runLocalScan() {
    setError("");
    setIsRunning(true);
    try {
      const response = await invoke<RunLocalImportResponse>("run_local_import", {
        request: {
          rootPath: autoDetectRoot ? "" : rootPath,
          strictOffline,
          allowOfficialApiExceptions,
          settings: scanSettings,
        },
      });
      setScanResult(response.scan);
      setImportSession(response.importSession);
      await loadRecentScans();
    } catch (e) {
      setError(
        e instanceof Error ? e.message : "Failed to run local scan command.",
      );
    } finally {
      setIsRunning(false);
    }
  }

  const setGameOverride = (gameId: string, value: string) => {
    setGamePathOverrides((current) => ({ ...current, [gameId]: value }));
  };

  const setManualFallback = (gameId: string, value: string) => {
    setManualFallbackPaths((current) => ({ ...current, [gameId]: value }));
  };

  return (
    <main className="app-shell">
      <header className="hero">
        <div className="hero-copy">
          <p className="eyebrow">LOCAL • OFFLINE-FIRST • WINDOWS</p>
          <h1>GachaTracker Desktop</h1>
          <p className="subtitle">
            Scan local logs and cache files for WuWa, HSR, ZZZ, and Endfield.
          </p>
          <div className="hero-meta">
            <span className={`status-pill tone-${scanState.tone}`}>
              {scanState.label}
            </span>
            <span className="muted">Last scan: {latestScanLabel}</span>
          </div>
        </div>
        <div className="hero-stats">
          <div className="stat-card">
            <span>Games tracked</span>
            <strong>{totals.games || 4}</strong>
          </div>
          <div className="stat-card">
            <span>Ready sources</span>
            <strong>{totals.ready}</strong>
          </div>
          <div className="stat-card">
            <span>Findings</span>
            <strong>{totals.findings}</strong>
          </div>
          <div className="stat-card">
            <span>Diagnostics</span>
            <strong>{diagnostics.length}</strong>
          </div>
        </div>
      </header>

      <div className="dashboard-grid">
        <section className="panel panel-wide">
          <div className="panel-head">
            <h2>Import scan</h2>
            <p className="panel-kicker">Offline pipeline entrypoint</p>
          </div>
          <div className="controls">
            <label>
              Root path (optional)
              <input
                value={rootPath}
                onChange={(e) => setRootPath(e.currentTarget.value)}
                placeholder="C:\\Games"
                disabled={autoDetectRoot}
              />
            </label>
            <label className="checkbox-row">
              <input
                type="checkbox"
                checked={autoDetectRoot}
                onChange={(e) => setAutoDetectRoot(e.currentTarget.checked)}
              />
              Auto-detect install paths (all local drives)
            </label>
            <label className="checkbox-row">
              <input
                type="checkbox"
                checked={strictOffline}
                onChange={(e) => {
                  const checked = e.currentTarget.checked;
                  setStrictOffline(checked);
                  if (checked) {
                    setAllowOfficialApiExceptions(false);
                  }
                }}
              />
              Strict offline mode (recommended)
            </label>
            <label className="checkbox-row">
              <input
                type="checkbox"
                checked={allowOfficialApiExceptions}
                disabled={strictOffline}
                onChange={(e) => setAllowOfficialApiExceptions(e.target.checked)}
              />
              Allow official API exceptions (officially confirmed hosts only)
            </label>
            <button type="button" disabled={isRunning} onClick={runLocalScan}>
              {isRunning ? "Scanning local artifacts..." : "Run local scan"}
            </button>
          </div>
          <p className="muted">
            Unknown or community-discovered endpoints stay blocked until officially
            confirmed.
          </p>
          <div className="policy-summary">
            <strong>Current policy:</strong>{" "}
            {exceptionModeRequested
              ? "Exception mode requested (only officially confirmed hosts can be used)"
              : "Strict local-only (all online calls blocked)"}
          </div>
          <p className="muted">
            Path overrides configured: {configuredPathOverrides} | Manual fallbacks
            configured: {configuredFallbacks}
          </p>
          {error ? <p className="error">{error}</p> : null}
        </section>

        <section className="panel panel-wide">
          <div className="panel-head">
            <h2>Settings & manual fallback</h2>
            <p className="panel-kicker">Per-game path controls and recovery input</p>
          </div>
          <div className="settings-grid">
            {GAME_SETTINGS.map((game) => (
              <article key={game.id} className="settings-card">
                <h3>{game.label}</h3>
                <label>
                  Game path override
                  <input
                    value={gamePathOverrides[game.id] ?? ""}
                    onChange={(e) => setGameOverride(game.id, e.currentTarget.value)}
                    placeholder={`C:\\Path\\to\\${game.label}`}
                  />
                </label>
                <label>
                  Manual fallback file
                  <input
                    value={manualFallbackPaths[game.id] ?? ""}
                    onChange={(e) =>
                      setManualFallback(game.id, e.currentTarget.value)
                    }
                    placeholder="C:\\Path\\to\\Player.log or data_2"
                  />
                </label>
                <p className="muted">{game.fallbackHint}</p>
              </article>
            ))}
          </div>
          <p className="muted">
            Overrides only affect the selected game adapter. Leave empty to use
            automatic path discovery.
          </p>
        </section>

        <section className="panel">
          <div className="panel-head">
            <h2>Network behavior</h2>
            <p className="panel-kicker">Runtime guardrails</p>
          </div>
          {scanResult?.networkPolicy ? (
            <div className="policy-summary">
              <p>
                <strong>Mode:</strong> {scanResult.networkPolicy.mode}
              </p>
              <p>{scanResult.networkPolicy.message}</p>
              <p className="muted">
                Online calls allowed:{" "}
                {scanResult.networkPolicy.onlineCallsAllowed ? "yes" : "no"} |
                Approved candidates: {scanResult.networkPolicy.approvedCandidates} |
                Blocked candidates: {scanResult.networkPolicy.blockedCandidates}
              </p>
              <p className="muted">
                Confirmed API hosts:{" "}
                {scanResult.networkPolicy.approvedHosts.length
                  ? scanResult.networkPolicy.approvedHosts.join(", ")
                  : "none configured"}
              </p>
            </div>
          ) : (
            <p className="empty-state">
              No scan yet. Default runtime policy is strict local-only; API
              exceptions require official confirmation.
            </p>
          )}
        </section>

        <section className="panel">
          <div className="panel-head">
            <h2>Data completeness</h2>
            <p className="panel-kicker">Coverage quality by latest scan</p>
          </div>
          <ul className="notes">
            <li className="note note-info">
              <span className="chip">full</span>
              <span>{completenessSummary.full} game(s)</span>
            </li>
            <li className="note note-warning">
              <span className="chip">partial</span>
              <span>{completenessSummary.partial} game(s)</span>
            </li>
            <li className="note note-warning">
              <span className="chip">none</span>
              <span>{completenessSummary.none} game(s)</span>
            </li>
          </ul>
          <p className="muted">
            Partial/none means local artifacts were found but full pull-history data
            is still incomplete.
          </p>
        </section>

        <section className="panel">
          <div className="panel-head">
            <h2>Diagnostics & notes</h2>
            <p className="panel-kicker">Importer signals</p>
          </div>
          {diagnostics.length ? (
            <ul className="notes">
              {diagnostics.map((diagnostic) => (
                <li key={diagnostic.id} className={`note note-${diagnostic.severity}`}>
                  <span className="chip">{diagnostic.severity}</span>
                  <span>{diagnostic.message}</span>
                </li>
              ))}
            </ul>
          ) : (
            <p className="empty-state">No diagnostics from the latest session.</p>
          )}
        </section>

        <section className="panel panel-wide">
          <div className="panel-head">
            <h2>Troubleshooting details</h2>
            <p className="panel-kicker">Path checks, fallback state, and next steps</p>
          </div>
          {troubleshooting.length ? (
            <div className="troubleshooting-grid">
              {troubleshooting.map((item) => (
                <article key={item.gameId} className="troubleshooting-card">
                  <div className="card-top">
                    <h3>{GAME_LABELS[item.gameId] ?? item.gameId}</h3>
                    <span className={`chip chip-${item.status}`}>{item.status}</span>
                  </div>
                  <p>{item.note}</p>
                  <div className="meta-row">
                    <span>Completeness: {item.completeness}</span>
                    <span>
                      Override: {item.rootOverrideApplied ? "enabled" : "default"}
                    </span>
                    <span>
                      Manual fallback:{" "}
                      {manualFallbackStatusLabel[item.manualFallbackStatus] ??
                        item.manualFallbackStatus}
                    </span>
                  </div>
                  <div className="troubleshooting-block">
                    <strong>Effective root:</strong>{" "}
                    <span className="code">{item.effectiveRootPath}</span>
                  </div>
                  <div className="troubleshooting-block">
                    <strong>Manual fallback path:</strong>{" "}
                    <span className="code">
                      {item.configuredManualFallbackPath ?? "not configured"}
                    </span>
                  </div>
                  <div className="troubleshooting-block">
                    <strong>Detected sources:</strong>{" "}
                    {item.detectedSourceFiles.length ? (
                      <span className="code">
                        {item.detectedSourceFiles.join(" | ")}
                      </span>
                    ) : (
                      <span className="muted">none</span>
                    )}
                  </div>
                  <div className="troubleshooting-block">
                    <strong>Checked path hints:</strong>
                    <ul className="mini-list">
                      {item.checkedPathHints.map((hint) => (
                        <li key={hint} className="code">
                          {hint}
                        </li>
                      ))}
                    </ul>
                  </div>
                  <div className="troubleshooting-block">
                    <strong>Next steps:</strong>
                    <ul className="mini-list">
                      {item.nextSteps.map((step) => (
                        <li key={step}>{step}</li>
                      ))}
                    </ul>
                  </div>
                </article>
              ))}
            </div>
          ) : (
            <p className="empty-state">
              Run a scan to populate per-game troubleshooting guidance.
            </p>
          )}
        </section>

        <section className="panel panel-wide">
          <div className="panel-head">
            <h2>Per-game status</h2>
            <p className="panel-kicker">Coverage across supported titles</p>
          </div>
          <div className="game-grid">
            {(importSession?.gameResults ?? []).map((game) => (
              <article key={game.gameId} className="game-card">
                <div className="card-top">
                  <h3>{game.gameId}</h3>
                  <span className={`chip chip-${game.status}`}>{game.status}</span>
                </div>
                <p>{game.note}</p>
                <div className="meta-row">
                  <span>Completeness: {game.completeness}</span>
                  <span>Files: {game.detectedFiles}</span>
                  <span>Findings: {game.findingsCount}</span>
                </div>
              </article>
            ))}
            {!scanResult ? (
              <p className="empty-state">Run a scan to populate game status cards.</p>
            ) : null}
          </div>
        </section>

        <section className="panel panel-wide">
          <div className="panel-head">
            <h2>Scan findings</h2>
            <p className="panel-kicker">Sanitized extraction output</p>
          </div>
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>Game</th>
                  <th>Type</th>
                  <th>Source</th>
                  <th>Value (sanitized)</th>
                  <th>Source file</th>
                </tr>
              </thead>
              <tbody>
                {(importSession?.pulls ?? []).map((pull) => (
                  <tr key={pull.id}>
                    <td>{pull.gameId}</td>
                    <td>{pull.kind}</td>
                    <td>{pull.sourceType}</td>
                    <td className="code">{pull.value}</td>
                    <td className="code">{pull.sourceFile}</td>
                  </tr>
                ))}
                {!importSession?.pulls.length ? (
                  <tr>
                    <td colSpan={5} className="muted center">
                      No findings yet.
                    </td>
                  </tr>
                ) : null}
              </tbody>
            </table>
          </div>
        </section>

        <section className="panel panel-wide">
          <div className="panel-head">
            <h2>Recent scans</h2>
            <p className="panel-kicker">Local history (latest 8)</p>
          </div>
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>ID</th>
                  <th>Timestamp</th>
                  <th>Root path</th>
                  <th>Offline</th>
                  <th>API exceptions</th>
                  <th>Total findings</th>
                </tr>
              </thead>
              <tbody>
                {recentScans.map((scan) => (
                  <tr key={scan.id}>
                    <td>{scan.id}</td>
                    <td>{formatTimestamp(scan.scannedAt)}</td>
                    <td className="code">{scan.rootPath}</td>
                    <td>{scan.strictOffline ? "yes" : "no"}</td>
                    <td>{scan.allowOfficialApiExceptions ? "yes" : "no"}</td>
                    <td>{scan.totalFindings}</td>
                  </tr>
                ))}
                {!recentScans.length ? (
                  <tr>
                    <td colSpan={6} className="muted center">
                      No scans in local history.
                    </td>
                  </tr>
                ) : null}
              </tbody>
            </table>
          </div>
        </section>
      </div>
    </main>
  );
}

export default App;
