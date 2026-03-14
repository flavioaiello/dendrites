use anyhow::Result;
use std::path::Path;

use super::analyze::{CallInfo, LiveDependency, ScanResult};

/// A trait that defines how to parse source files into generic domain intelligence artifacts
/// mapping code structures into the `dendrites` graph-based boundary systems.
///
/// Implementations of this trait are language-specific (e.g. `RustSynScanner`, `TypeScriptTreeSitterScanner`).
pub trait AstScanner {
    /// Extracts module/package dependencies (e.g. `use`, `import`) to build the cross-cutting graph.
    fn extract_live_dependencies(
        &self,
        file_path: &Path,
        source_code: &str,
    ) -> Result<Vec<LiveDependency>>;

    /// Parses the file to find types (classes/structs), enums, modules, and their behaviors (methods/functions)
    fn scan_file(&self, file_path: &Path, source_code: &str) -> Result<ScanResult>;

    /// Extracts function/method call edges from the source code.
    fn extract_calls(&self, file_path: &Path, source_code: &str) -> Result<Vec<CallInfo>>;
}
