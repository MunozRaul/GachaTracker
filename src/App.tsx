import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";
import {
  type ImportSession,
  type RunLocalImportResponse,
  type ScanSettings,
} from "./domain/models";

const GAME_SETTINGS = [
  { id: "wuthering-waves", label: "Wuthering Waves" },
  { id: "honkai-star-rail", label: "Honkai: Star Rail" },
  { id: "zenless-zone-zero", label: "Zenless Zone Zero" },
  { id: "endfield", label: "Endfield" },
] as const;

const GAME_LABELS = Object.fromEntries(
  GAME_SETTINGS.map((game) => [game.id, game.label]),
) as Record<string, string>;

const DEFAULT_SETTINGS: ScanSettings = {
  gamePaths: {},
  manualFallbackPaths: {},
};

const WEAPON_BANNER_KEYWORDS = ["weapon", "light cone", "w-engine", "armament"];

type ViewTab = "games" | "history";
type ConveneType = "featured-resonator" | "featured-weapon";

function isWeaponBanner(name: string) {
  const normalized = name.toLowerCase();
  return WEAPON_BANNER_KEYWORDS.some((keyword) => normalized.includes(keyword));
}

function App() {
  const [activeTab, setActiveTab] = useState<ViewTab>("games");
  const [selectedGameId, setSelectedGameId] = useState<string>(GAME_SETTINGS[0].id);
  const [selectedConveneType, setSelectedConveneType] =
    useState<ConveneType>("featured-resonator");
  const [runningGameId, setRunningGameId] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [importSession, setImportSession] = useState<ImportSession | null>(null);
  const [lastScanNotes, setLastScanNotes] = useState<string[]>([]);

  const isRunning = runningGameId !== null;

  useEffect(() => {
    void (async () => {
      try {
        const latest = await invoke<ImportSession | null>("get_latest_import_session");
        if (latest) {
          setImportSession(latest);
          setLastScanNotes(["Loaded latest local import session."]);
        }
      } catch {
        // best-effort bootstrap; ignore load errors here
      }
    })();
  }, []);

  const bannerNameById = useMemo(
    () =>
      new Map(
        (importSession?.banners ?? []).map((banner) => [banner.id, banner.name]),
      ),
    [importSession?.banners],
  );

  const sortedPulls = useMemo(() => importSession?.pulls ?? [], [importSession?.pulls]);

  const selectedGamePulls = useMemo(
    () => sortedPulls.filter((pull) => pull.gameId === selectedGameId),
    [selectedGameId, sortedPulls],
  );

  const actualGamePulls = useMemo(
    () =>
      selectedGamePulls.filter((pull) => pull.kind === "history_pull"),
    [selectedGamePulls],
  );

  const resonatorRows = useMemo(
    () =>
      actualGamePulls.filter((pull) => {
        const bannerName = bannerNameById.get(pull.bannerId) ?? pull.bannerId;
        return !isWeaponBanner(bannerName);
      }),
    [actualGamePulls, bannerNameById],
  );

  const weaponRows = useMemo(
    () =>
      actualGamePulls.filter((pull) => {
        const bannerName = bannerNameById.get(pull.bannerId) ?? pull.bannerId;
        return isWeaponBanner(bannerName);
      }),
    [actualGamePulls, bannerNameById],
  );

  useEffect(() => {
    if (
      selectedConveneType === "featured-resonator" &&
      !resonatorRows.length &&
      weaponRows.length
    ) {
      setSelectedConveneType("featured-weapon");
    } else if (
      selectedConveneType === "featured-weapon" &&
      !weaponRows.length &&
      resonatorRows.length
    ) {
      setSelectedConveneType("featured-resonator");
    }
  }, [resonatorRows.length, selectedConveneType, weaponRows.length]);

  const visibleRows = useMemo(
    () =>
      selectedConveneType === "featured-weapon" ? weaponRows : resonatorRows,
    [resonatorRows, selectedConveneType, weaponRows],
  );

  const displayRows = useMemo(
    () => visibleRows.filter((pull) => pull.rarity !== 3),
    [visibleRows],
  );

  const fiveStarSummary = useMemo(() => {
    // The imported pull stream is newest -> oldest. Reverse once so pity counts
    // are computed from older pulls toward newer pulls.
    const chronologicalRows = [...visibleRows].reverse();

    let pullsSinceLastFive = 0;
    const summary = [] as Array<{
      id: string;
      itemName: string;
      pulledAt: string;
      pullsToFive: number;
    }>;

    for (const pull of chronologicalRows) {
      pullsSinceLastFive += 1;
      if (pull.rarity === 5) {
        summary.push({
          id: pull.id,
          itemName: pull.itemName ?? pull.value,
          pulledAt: pull.pulledAt ?? "-",
          pullsToFive: pullsSinceLastFive,
        });
        pullsSinceLastFive = 0;
      }
    }

    return summary.reverse();
  }, [visibleRows]);

  const summaryChipClass = (pullsToFive: number) => {
    if (pullsToFive <= 40) {
      return "chip-ready";
    }
    if (pullsToFive <= 65) {
      return "chip-warn";
    }
    return "chip-danger";
  };

  async function runScanForGame(gameId: string) {
    setError("");
    setSelectedGameId(gameId);
    setActiveTab("history");
    setRunningGameId(gameId);

    try {
      const response = await invoke<RunLocalImportResponse>("run_local_import", {
        request: {
          rootPath: "",
          strictOffline: false,
          allowOfficialApiExceptions: true,
          settings: DEFAULT_SETTINGS,
        },
      });
      const nextHasHistoryForGame = response.importSession.pulls.some(
        (pull) => pull.kind === "history_pull" && pull.gameId === gameId,
      );
      const currentHasHistoryForGame = (importSession?.pulls ?? []).some(
        (pull) => pull.kind === "history_pull" && pull.gameId === gameId,
      );

      if (!nextHasHistoryForGame && currentHasHistoryForGame) {
        setLastScanNotes([
          ...(response.scan.notes ?? []),
          "Latest scan returned no pulls for this game. Showing last known local results.",
        ]);
      } else {
        setImportSession(response.importSession);
        setLastScanNotes(response.scan.notes ?? []);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to load pull history.");
    } finally {
      setRunningGameId(null);
    }
  }

  return (
    <main className="app-shell compact-shell">
      <header className="hero compact-hero">
        <div className="hero-copy">
          <p className="eyebrow">LOCAL TRACKER</p>
          <h1>GachaTracker Desktop</h1>
          <p className="subtitle">
            Pick a game to import pull history. Auto-detect and official API
            exceptions are enabled by default.
          </p>
        </div>
      </header>

      <nav className="view-tabs" aria-label="Main views">
        <button
          type="button"
          className={`tab-button ${activeTab === "games" ? "active" : ""}`}
          onClick={() => setActiveTab("games")}
        >
          Games
        </button>
        <button
          type="button"
          className={`tab-button ${activeTab === "history" ? "active" : ""}`}
          onClick={() => setActiveTab("history")}
          disabled={!importSession && !isRunning}
        >
          Pull history
        </button>
      </nav>

      {activeTab === "games" ? (
        <section className="panel panel-wide game-select-panel">
          <div className="panel-head">
            <h2>Select game</h2>
            <p className="panel-kicker">
              Clicking a game starts the scan and opens pull history.
            </p>
          </div>
          <div className="game-picker-grid">
            {GAME_SETTINGS.map((game) => (
              <button
                key={game.id}
                type="button"
                className="game-picker-button"
                onClick={() => runScanForGame(game.id)}
                disabled={isRunning}
              >
                <span>{game.label}</span>
                <small>
                  {isRunning && runningGameId === game.id
                    ? "Loading..."
                    : "Open pull history"}
                </small>
              </button>
            ))}
          </div>
          {error ? <p className="error">{error}</p> : null}
        </section>
      ) : (
        <section className="panel panel-wide history-panel">
          <div className="history-layout">
            <aside className="history-sidebar">
              <h3>{GAME_LABELS[selectedGameId] ?? selectedGameId}</h3>
              <button
                type="button"
                className="secondary-action"
                onClick={() => runScanForGame(selectedGameId)}
                disabled={isRunning}
              >
                {isRunning ? "Refreshing..." : "Refresh pulls"}
              </button>

              <div className="convene-buttons">
                <button
                  type="button"
                  className={`convene-option ${
                    selectedConveneType === "featured-resonator" ? "active" : ""
                  }`}
                  onClick={() => setSelectedConveneType("featured-resonator")}
                >
                  Featured resonator
                </button>
                <button
                  type="button"
                  className={`convene-option ${
                    selectedConveneType === "featured-weapon" ? "active" : ""
                  }`}
                  onClick={() => setSelectedConveneType("featured-weapon")}
                >
                  Featured weapon
                </button>
              </div>

              {lastScanNotes.length ? (
                <ul className="scan-note-list">
                  {lastScanNotes.map((note) => (
                    <li key={note} className="muted small-note">
                      {note}
                    </li>
                  ))}
                </ul>
              ) : null}
              {error ? <p className="error">{error}</p> : null}
            </aside>

            <div className="history-table-container">
              <div className="panel-head">
                <h2>Pull history</h2>
                <p className="panel-kicker">
                  3★ hidden • sorted by Pull ID (desc) with timestamp fallback
                </p>
              </div>
              <div className="table-wrap">
                <table>
                  <thead>
                    <tr>
                      <th>Item</th>
                      <th>Rarity</th>
                      <th>Type</th>
                      <th>Pulled at</th>
                    </tr>
                  </thead>
                  <tbody>
                    {displayRows.map((pull) => (
                      <tr key={pull.id}>
                        <td>{pull.itemName ?? pull.value}</td>
                        <td
                          className={
                            pull.rarity === 5
                              ? "rarity-cell rarity-5"
                              : pull.rarity === 4
                                ? "rarity-cell rarity-4"
                                : "rarity-cell"
                          }
                        >
                          {pull.rarity ? `★${pull.rarity}` : "-"}
                        </td>
                        <td>{pull.itemTypeName ?? pull.kind}</td>
                        <td>{pull.pulledAt ?? "-"}</td>
                      </tr>
                    ))}
                    {!displayRows.length ? (
                      <tr>
                        <td colSpan={4} className="muted center">
                          {isRunning
                            ? "Loading pull history..."
                            : "No actual pull-history rows found for this game/convene yet."}
                        </td>
                      </tr>
                    ) : null}
                  </tbody>
                </table>
              </div>
            </div>

            <aside className="summary-sidebar">
              <div className="panel-head">
                <h2>5★ summary</h2>
                <p className="panel-kicker">
                  Pulls needed between consecutive 5★ entries
                </p>
              </div>
              {fiveStarSummary.length ? (
                <div className="five-star-list">
                  {fiveStarSummary.map((entry) => (
                    <article key={entry.id} className="five-star-card">
                      <strong>{entry.itemName}</strong>
                      <span className="muted">{entry.pulledAt}</span>
                      <span className={`chip ${summaryChipClass(entry.pullsToFive)}`}>
                        {entry.pullsToFive} pulls to 5★
                      </span>
                    </article>
                  ))}
                </div>
              ) : (
                <p className="empty-state">
                  No 5★ pulls found for the selected convene yet.
                </p>
              )}
            </aside>
          </div>
        </section>
      )}
    </main>
  );
}

export default App;
