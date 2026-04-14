export type ImportStatus = "ready" | "missing" | "no-history";
export type CompletenessMarker = "none" | "partial" | "full";
export type SourceType = "log" | "cache" | "network" | "unknown";
export type DiagnosticSeverity = "info" | "warning" | "error";
export type ManualFallbackStatus =
  | "not-configured"
  | "loaded"
  | "missing"
  | "read-error";

export type GameScanResult = {
  gameId: string;
  status: ImportStatus;
  completeness: CompletenessMarker;
  detectedFiles: number;
  findingsCount: number;
  note: string;
};

export type ScanFinding = {
  gameId: string;
  sourceFile: string;
  kind: string;
  value: string;
};

export type ScanSettings = {
  gamePaths: Record<string, string>;
  manualFallbackPaths: Record<string, string>;
};

export type GameTroubleshooting = {
  gameId: string;
  effectiveRootPath: string;
  rootOverrideApplied: boolean;
  configuredManualFallbackPath?: string;
  manualFallbackStatus: ManualFallbackStatus;
  checkedPathHints: string[];
  detectedSourceFiles: string[];
  status: ImportStatus;
  completeness: CompletenessMarker;
  note: string;
  nextSteps: string[];
};

export type NetworkPolicyState = {
  mode: "strict-local-only" | "official-api-exceptions";
  onlineCallsAllowed: boolean;
  approvedHosts: string[];
  approvedCandidates: number;
  blockedCandidates: number;
  message: string;
};

export type ScanResponse = {
  scanId: number;
  scannedAt: string;
  rootPath: string;
  strictOffline: boolean;
  allowOfficialApiExceptions: boolean;
  networkPolicy: NetworkPolicyState;
  gameResults: GameScanResult[];
  findings: ScanFinding[];
  historyPulls: HistoryPullRow[];
  notes: string[];
  troubleshooting: GameTroubleshooting[];
};

export type HistoryPullRow = {
  gameId: string;
  bannerId: string;
  bannerName: string;
  itemName: string;
  itemTypeName: string;
  rarity: number;
  pulledAt: string;
  pullId: string;
  sourceUrl: string;
};

export type ScanSummary = {
  id: number;
  scannedAt: string;
  rootPath: string;
  strictOffline: boolean;
  allowOfficialApiExceptions: boolean;
  totalFindings: number;
};

export type Pull = {
  id: string;
  gameId: string;
  bannerId: string;
  sourceFile: string;
  sourceType: SourceType;
  kind: string;
  value: string;
  itemName?: string;
  itemTypeName?: string;
  rarity?: number;
  pulledAt?: string;
};

export type Banner = {
  id: string;
  gameId: string;
  name: string;
  pullType: "history-url" | "possible-source";
};

export type SourceMetadata = {
  id: string;
  gameId: string;
  path: string;
  sourceType: SourceType;
  findings: number;
};

export type ImportDiagnostic = {
  id: string;
  severity: DiagnosticSeverity;
  message: string;
  gameId?: string;
};

export type ImportSession = {
  id: number;
  scannedAt: string;
  rootPath: string;
  strictOffline: boolean;
  allowOfficialApiExceptions: boolean;
  gameResults: GameScanResult[];
  completenessByGame: Record<string, CompletenessMarker>;
  pulls: Pull[];
  banners: Banner[];
  sourceMetadata: SourceMetadata[];
  diagnostics: ImportDiagnostic[];
};

export type RunLocalImportResponse = {
  scan: ScanResponse;
  importSession: ImportSession;
};

const sourceTypeFromPath = (path: string): SourceType => {
  const normalized = path.toLowerCase();
  if (normalized.includes("cache") || normalized.endsWith("data_2")) {
    return "cache";
  }
  if (normalized.endsWith(".log")) {
    return "log";
  }
  return "unknown";
};

const bannerNameByGame: Record<string, string> = {
  "wuthering-waves": "Convene",
  "honkai-star-rail": "Warp",
  "zenless-zone-zero": "Signal Search",
  endfield: "Recruitment",
};

const bannerTypeFromKind = (kind: string): Banner["pullType"] =>
  kind === "possible_history_source" ? "possible-source" : "history-url";

const bannerIdFor = (gameId: string, kind: string): string =>
  `${gameId}:${bannerTypeFromKind(kind)}`;

export function toImportSession(scan: ScanResponse): ImportSession {
  const pulls: Pull[] =
    scan.historyPulls.length > 0
      ? scan.historyPulls.map((row, index) => ({
          id: row.pullId || `${scan.scanId}:${index}`,
          gameId: row.gameId,
          bannerId: row.bannerId,
          sourceFile: row.sourceUrl,
          sourceType: "network",
          kind: "history_pull",
          value: row.itemName,
          itemName: row.itemName,
          itemTypeName: row.itemTypeName,
          rarity: row.rarity,
          pulledAt: row.pulledAt,
        }))
      : scan.findings.map((finding, index) => ({
          id: `${scan.scanId}:${index}`,
          gameId: finding.gameId,
          bannerId: bannerIdFor(finding.gameId, finding.kind),
          sourceFile: finding.sourceFile,
          sourceType: sourceTypeFromPath(finding.sourceFile),
          kind: finding.kind,
          value: finding.value,
        }));

  const bannersById = new Map<string, Banner>();
  for (const pull of pulls) {
    if (!bannersById.has(pull.bannerId)) {
      bannersById.set(pull.bannerId, {
        id: pull.bannerId,
        gameId: pull.gameId,
        name:
          scan.historyPulls.find((row) => row.bannerId === pull.bannerId)
            ?.bannerName ??
          bannerNameByGame[pull.gameId] ??
          "Unknown banner",
        pullType: pull.kind === "history_pull" ? "history-url" : bannerTypeFromKind(pull.kind),
      });
    }
  }
  for (const game of scan.gameResults) {
    const defaultBannerId = `${game.gameId}:history-url`;
    if (!bannersById.has(defaultBannerId)) {
      bannersById.set(defaultBannerId, {
        id: defaultBannerId,
        gameId: game.gameId,
        name: bannerNameByGame[game.gameId] ?? "Unknown banner",
        pullType: "history-url",
      });
    }
  }

  const sourceMetadataById = new Map<string, SourceMetadata>();
  for (const pull of pulls) {
    const id = `${pull.gameId}:${pull.sourceFile}`;
    const existing = sourceMetadataById.get(id);
    if (existing) {
      existing.findings += 1;
      continue;
    }
    sourceMetadataById.set(id, {
      id,
      gameId: pull.gameId,
      path: pull.sourceFile,
      sourceType: pull.sourceType,
      findings: 1,
    });
  }

  const diagnostics: ImportDiagnostic[] = [
    ...scan.notes.map((message, index) => ({
      id: `note:${index}`,
      severity: "info" as const,
      message,
    })),
    ...scan.gameResults
      .filter((game) => game.status !== "ready")
      .map((game) => ({
        id: `status:${game.gameId}`,
        severity: "warning" as const,
        message: game.note,
        gameId: game.gameId,
      })),
  ];

  const completenessByGame = Object.fromEntries(
    scan.gameResults.map((game) => [game.gameId, game.completeness]),
  ) as Record<string, CompletenessMarker>;

  return {
    id: scan.scanId,
    scannedAt: scan.scannedAt,
    rootPath: scan.rootPath,
    strictOffline: scan.strictOffline,
    allowOfficialApiExceptions: scan.allowOfficialApiExceptions,
    gameResults: scan.gameResults,
    completenessByGame,
    pulls,
    banners: Array.from(bannersById.values()),
    sourceMetadata: Array.from(sourceMetadataById.values()),
    diagnostics,
  };
}
