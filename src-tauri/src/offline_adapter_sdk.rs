use std::path::Path;

use super::{GameScanResult, ScanFinding};

pub(crate) struct OfflineAdapterScan {
    pub(crate) findings: Vec<ScanFinding>,
    pub(crate) game_result: GameScanResult,
}

pub(crate) trait OfflineAdapter {
    fn id(&self) -> &'static str;
    fn scan(&self, root: &Path) -> Result<OfflineAdapterScan, String>;
}

pub(crate) struct FunctionOfflineAdapter {
    id: &'static str,
    scan_fn: fn(&Path) -> Result<(Vec<ScanFinding>, GameScanResult), String>,
}

impl FunctionOfflineAdapter {
    pub(crate) fn new(
        id: &'static str,
        scan_fn: fn(&Path) -> Result<(Vec<ScanFinding>, GameScanResult), String>,
    ) -> Self {
        Self { id, scan_fn }
    }
}

impl OfflineAdapter for FunctionOfflineAdapter {
    fn id(&self) -> &'static str {
        self.id
    }

    fn scan(&self, root: &Path) -> Result<OfflineAdapterScan, String> {
        let (findings, game_result) = (self.scan_fn)(root)?;
        Ok(OfflineAdapterScan {
            findings,
            game_result,
        })
    }
}

pub(crate) struct OfflineAdapterRegistry {
    adapters: Vec<Box<dyn OfflineAdapter>>,
}

impl OfflineAdapterRegistry {
    pub(crate) fn new() -> Self {
        Self {
            adapters: Vec::new(),
        }
    }

    pub(crate) fn register<A: OfflineAdapter + 'static>(&mut self, adapter: A) {
        self.adapters.push(Box::new(adapter));
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &dyn OfflineAdapter> {
        self.adapters.iter().map(Box::as_ref)
    }
}
