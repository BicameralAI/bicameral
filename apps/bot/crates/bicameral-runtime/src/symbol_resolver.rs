//! Rust symbol resolver — tree-sitter-based symbol grounding.
//!
//! Implements the bot-owned Rust `SymbolResolver` per issue #50 and #54's
//! decision. Carries forward lessons from BicameralAI/bicameral-mcp#399:
//!
//! - Rust `tags.scm` capture kinds are audited and mapped explicitly.
//! - Only verified evidence states may produce `SymbolOccurrence` records.
//! - Unsupported languages return `Unsupported`, not approximate evidence.
//! - Protocol-shaped MCP compatibility without MCP-internal code reuse.
//!
//! Capture-kind audit (tree-sitter-rust TAGS_QUERY):
//!
//! | Upstream capture          | AST nodes                                    | Mapped SymbolKind |
//! |---------------------------|----------------------------------------------|-------------------|
//! | `@definition.class`       | struct_item, enum_item, union_item, type_item | Struct/Enum/Union/TypeAlias |
//! | `@definition.method`      | function_item inside declaration_list         | Method            |
//! | `@definition.function`    | function_item (top-level)                    | Function          |
//! | `@definition.interface`   | trait_item                                   | Trait             |
//! | `@definition.module`      | mod_item                                     | Module            |
//! | `@definition.macro`       | macro_definition                             | Macro             |
//! | `@reference.call`         | call_expression                              | (reference, not indexed) |
//! | `@reference.implementation` | impl_item                                  | Impl              |

use bicameral_api::evidence::{
    GraphEvidenceState, GraphSnapshot, RustCaptureKind, SymbolKind, SymbolLanguage,
    SymbolOccurrence, SymbolResolutionResult,
};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use streaming_iterator::StreamingIterator;

/// Parser version string embedded in structural evidence.
const PARSER_VERSION: &str = "tree-sitter-rust-0.23";

/// Index version — bump when resolver logic changes output for same input.
#[allow(dead_code)]
const SYMBOL_INDEX_VERSION: &str = "symbol-resolve-v1";

/// Resolves Rust symbols from source files using tree-sitter.
pub struct RustSymbolResolver {
    workspace_root: PathBuf,
    snapshot: GraphSnapshot,
}

impl RustSymbolResolver {
    pub fn new(workspace_root: &Path, snapshot: GraphSnapshot) -> Self {
        Self {
            workspace_root: workspace_root.to_path_buf(),
            snapshot,
        }
    }

    /// Resolve all definition symbols in a Rust source file.
    ///
    /// Returns `SymbolResolutionResult` with evidence states per ADR-0010.
    /// Only committed, non-dirty files produce `Verified` occurrences.
    pub fn resolve_file(&self, rel_path: &str) -> SymbolResolutionResult {
        if !rel_path.ends_with(".rs") {
            return self
                .unsupported_result("Only .rs files are supported by the Rust symbol resolver");
        }

        let full_path = self.workspace_root.join(rel_path);

        if !full_path.exists() {
            return SymbolResolutionResult {
                graph_snapshot_id: self.snapshot.graph_snapshot_id.clone(),
                language: SymbolLanguage::Rust,
                occurrences: vec![],
                evidence_state: GraphEvidenceState::NotFound,
                detail: Some(format!("File not found: {rel_path}")),
            };
        }

        if !full_path.is_file() {
            return self.unsupported_result("Path is not a regular file");
        }

        let content = match std::fs::read_to_string(&full_path) {
            Ok(c) => c,
            Err(e) => {
                return self.unsupported_result(&format!("Cannot read file: {e}"));
            }
        };

        if self.is_dirty(rel_path) {
            return SymbolResolutionResult {
                graph_snapshot_id: self.snapshot.graph_snapshot_id.clone(),
                language: SymbolLanguage::Rust,
                occurrences: vec![],
                evidence_state: GraphEvidenceState::UnknownStale,
                detail: Some("Working tree differs from committed snapshot".to_string()),
            };
        }

        let occurrences = self.parse_symbols(rel_path, &content);

        SymbolResolutionResult {
            graph_snapshot_id: self.snapshot.graph_snapshot_id.clone(),
            language: SymbolLanguage::Rust,
            evidence_state: GraphEvidenceState::Verified,
            occurrences,
            detail: None,
        }
    }

    /// Find all occurrences of a named symbol across Rust files in the workspace.
    ///
    /// Searches `.rs` files, returning verified `SymbolOccurrence` records for
    /// each definition matching `symbol_name`. If no matches are found, returns
    /// `NotFound`. If the workspace has dirty files, affected symbols are excluded.
    pub fn find_symbol(&self, symbol_name: &str) -> SymbolResolutionResult {
        let rs_files = self.collect_rs_files();
        let mut all_occurrences = Vec::new();
        let mut any_stale = false;

        for rel_path in &rs_files {
            let full_path = self.workspace_root.join(rel_path);
            let content = match std::fs::read_to_string(&full_path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            if self.is_dirty(rel_path) {
                any_stale = true;
                continue;
            }

            let symbols = self.parse_symbols(rel_path, &content);
            for occ in symbols {
                if occ.symbol_name == symbol_name {
                    all_occurrences.push(occ);
                }
            }
        }

        if all_occurrences.is_empty() {
            let state = if any_stale {
                GraphEvidenceState::UnknownStale
            } else {
                GraphEvidenceState::NotFound
            };
            return SymbolResolutionResult {
                graph_snapshot_id: self.snapshot.graph_snapshot_id.clone(),
                language: SymbolLanguage::Rust,
                occurrences: vec![],
                evidence_state: state,
                detail: Some(format!("Symbol '{symbol_name}' not found in workspace")),
            };
        }

        if all_occurrences.len() > 1 {
            return SymbolResolutionResult {
                graph_snapshot_id: self.snapshot.graph_snapshot_id.clone(),
                language: SymbolLanguage::Rust,
                occurrences: all_occurrences,
                evidence_state: GraphEvidenceState::Ambiguous,
                detail: Some(format!(
                    "Multiple definitions of '{symbol_name}' found; caller must disambiguate"
                )),
            };
        }

        SymbolResolutionResult {
            graph_snapshot_id: self.snapshot.graph_snapshot_id.clone(),
            language: SymbolLanguage::Rust,
            occurrences: all_occurrences,
            evidence_state: GraphEvidenceState::Verified,
            detail: None,
        }
    }

    /// Resolve symbols for a non-Rust language — returns `Unsupported`.
    pub fn unsupported_language(&self, language: &str) -> SymbolResolutionResult {
        SymbolResolutionResult {
            graph_snapshot_id: self.snapshot.graph_snapshot_id.clone(),
            language: SymbolLanguage::Unsupported,
            occurrences: vec![],
            evidence_state: GraphEvidenceState::Unsupported,
            detail: Some(format!(
                "Language '{language}' is not supported by the symbol resolver. \
                 Only Rust is supported in this slice."
            )),
        }
    }

    fn parse_symbols(&self, rel_path: &str, content: &str) -> Vec<SymbolOccurrence> {
        let mut parser = tree_sitter::Parser::new();
        let language = tree_sitter_rust::LANGUAGE;
        parser
            .set_language(&language.into())
            .expect("tree-sitter-rust language");

        let tree = match parser.parse(content, None) {
            Some(t) => t,
            None => return vec![],
        };

        let query = match tree_sitter::Query::new(&language.into(), tree_sitter_rust::TAGS_QUERY) {
            Ok(q) => q,
            Err(_) => return vec![],
        };

        let mut cursor = tree_sitter::QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), content.as_bytes());

        let name_idx = query
            .capture_index_for_name("name")
            .expect("tags.scm must have @name capture");

        let mut occurrences = Vec::new();

        while let Some(m) = matches.next() {
            let mut capture_name_text: Option<String> = None;
            let mut def_node = None;
            let mut role_kind: Option<String> = None;

            for cap in m.captures {
                let cap_name = &query.capture_names()[cap.index as usize];
                if cap.index == name_idx {
                    capture_name_text = Some(
                        cap.node
                            .utf8_text(content.as_bytes())
                            .unwrap_or("")
                            .to_string(),
                    );
                } else {
                    def_node = Some(cap.node);
                    role_kind = Some(cap_name.to_string());
                }
            }

            let sym_name = match &capture_name_text {
                Some(n) if !n.is_empty() => n.clone(),
                _ => continue,
            };

            let node = match def_node {
                Some(n) => n,
                None => continue,
            };

            let rk = match &role_kind {
                Some(r) => r.as_str(),
                None => continue,
            };

            let (symbol_kind, capture_kind) = match map_capture_kind(rk, &node) {
                Some(pair) => pair,
                None => continue,
            };

            let start_line = node.start_position().row as u32 + 1;
            let end_line = node.end_position().row as u32 + 1;

            let node_text = node.utf8_text(content.as_bytes()).unwrap_or("");

            let content_hash = sha256_hex(node_text);
            let structural_hash = compute_structural_hash(&node);

            occurrences.push(SymbolOccurrence {
                graph_snapshot_id: self.snapshot.graph_snapshot_id.clone(),
                symbol_name: sym_name,
                qualified_name: None,
                symbol_kind,
                language: SymbolLanguage::Rust,
                file_path: rel_path.to_string(),
                line_start: start_line,
                line_end: end_line,
                content_hash,
                structural_hash,
                evidence_state: GraphEvidenceState::Verified,
                detail: None,
                parser_version: Some(PARSER_VERSION.to_string()),
                capture_kind: Some(capture_kind),
            });
        }

        occurrences
    }

    fn is_dirty(&self, path: &str) -> bool {
        let status = std::process::Command::new("git")
            .args(["diff", "--quiet", "HEAD", "--", path])
            .current_dir(&self.workspace_root)
            .status();

        match status {
            Ok(s) => !s.success(),
            Err(_) => true,
        }
    }

    fn unsupported_result(&self, detail: &str) -> SymbolResolutionResult {
        SymbolResolutionResult {
            graph_snapshot_id: self.snapshot.graph_snapshot_id.clone(),
            language: SymbolLanguage::Rust,
            occurrences: vec![],
            evidence_state: GraphEvidenceState::Unsupported,
            detail: Some(detail.to_string()),
        }
    }

    fn collect_rs_files(&self) -> Vec<String> {
        let output = std::process::Command::new("git")
            .args(["ls-files", "*.rs", "**/*.rs"])
            .current_dir(&self.workspace_root)
            .output();

        match output {
            Ok(o) if o.status.success() => {
                let text = String::from_utf8_lossy(&o.stdout);
                text.lines()
                    .filter(|l| !l.is_empty())
                    .map(|l| l.to_string())
                    .collect()
            }
            _ => vec![],
        }
    }
}

/// Map a tree-sitter capture name to `(SymbolKind, RustCaptureKind)`.
///
/// Returns `None` for captures we intentionally skip (e.g. `reference.call`
/// which is a reference, not a definition).
fn map_capture_kind(
    capture_name: &str,
    node: &tree_sitter::Node,
) -> Option<(SymbolKind, RustCaptureKind)> {
    match capture_name {
        "definition.class" => {
            let kind = match node.kind() {
                "struct_item" => SymbolKind::Struct,
                "enum_item" => SymbolKind::Enum,
                "union_item" => SymbolKind::Union,
                "type_item" => SymbolKind::TypeAlias,
                _ => SymbolKind::Struct,
            };
            Some((kind, RustCaptureKind::DefinitionClass))
        }
        "definition.method" => Some((SymbolKind::Method, RustCaptureKind::DefinitionMethod)),
        "definition.function" => Some((SymbolKind::Function, RustCaptureKind::DefinitionFunction)),
        "definition.interface" => Some((SymbolKind::Trait, RustCaptureKind::DefinitionInterface)),
        "definition.module" => Some((SymbolKind::Module, RustCaptureKind::DefinitionModule)),
        "definition.macro" => Some((SymbolKind::Macro, RustCaptureKind::DefinitionMacro)),
        "reference.implementation" => {
            Some((SymbolKind::Impl, RustCaptureKind::ReferenceImplementation))
        }
        // reference.call is a reference, not a definition — skip.
        "reference.call" => None,
        _ => None,
    }
}

/// Compute a structural hash from the tree-sitter node kind tree.
///
/// Walks the node's children recursively, hashing their `kind()` strings
/// and child counts. This captures the AST structure independent of
/// identifier names and whitespace.
fn compute_structural_hash(node: &tree_sitter::Node) -> String {
    let mut hasher = Sha256::new();
    hash_node_structure(&mut hasher, node);
    format!("{:x}", hasher.finalize())
}

fn hash_node_structure(hasher: &mut Sha256, node: &tree_sitter::Node) {
    hasher.update(node.kind().as_bytes());
    hasher.update(b":");
    hasher.update(node.child_count().to_string().as_bytes());
    hasher.update(b";");
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        hash_node_structure(hasher, &child);
    }
}

fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grounding::SnapshotResolver;
    use std::process::Command;

    fn setup_rust_fixture_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();

        Command::new("git")
            .args(["init"])
            .current_dir(p)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(p)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(p)
            .output()
            .unwrap();

        // Write a fixture Rust file with various symbol kinds.
        let fixture = r#"
pub struct Config {
    pub name: String,
    pub value: u32,
}

pub enum Status {
    Active,
    Inactive,
}

pub union RawData {
    int_val: i32,
    float_val: f32,
}

pub type UserId = u64;

pub trait Validator {
    fn validate(&self) -> bool;
}

impl Validator for Config {
    fn validate(&self) -> bool {
        !self.name.is_empty()
    }
}

impl Config {
    pub fn new(name: String, value: u32) -> Self {
        Self { name, value }
    }
}

pub fn process(input: &str) -> String {
    input.to_uppercase()
}

pub mod utils {
    pub fn helper() -> u32 {
        42
    }
}

macro_rules! my_macro {
    ($x:expr) => {
        println!("{}", $x);
    };
}
"#;

        std::fs::write(p.join("lib.rs"), fixture).unwrap();

        // Also write a non-Rust file to test unsupported paths.
        std::fs::write(p.join("readme.md"), "# Hello\n").unwrap();

        Command::new("git")
            .args(["add", "."])
            .current_dir(p)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init fixture"])
            .current_dir(p)
            .output()
            .unwrap();

        dir
    }

    #[test]
    fn resolves_all_definition_kinds() {
        let dir = setup_rust_fixture_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let snap = resolver.resolve_head().unwrap();
        let sym = RustSymbolResolver::new(dir.path(), snap.clone());

        let result = sym.resolve_file("lib.rs");
        assert_eq!(result.evidence_state, GraphEvidenceState::Verified);
        assert_eq!(result.language, SymbolLanguage::Rust);

        let names: Vec<&str> = result
            .occurrences
            .iter()
            .map(|o| o.symbol_name.as_str())
            .collect();

        assert!(names.contains(&"Config"), "should find struct Config");
        assert!(names.contains(&"Status"), "should find enum Status");
        assert!(names.contains(&"RawData"), "should find union RawData");
        assert!(names.contains(&"UserId"), "should find type alias UserId");
        assert!(names.contains(&"Validator"), "should find trait Validator");
        assert!(names.contains(&"process"), "should find fn process");
        assert!(names.contains(&"utils"), "should find mod utils");
        assert!(names.contains(&"my_macro"), "should find macro my_macro");
    }

    #[test]
    fn occurrences_carry_snapshot_identity() {
        let dir = setup_rust_fixture_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let snap = resolver.resolve_head().unwrap();
        let sym = RustSymbolResolver::new(dir.path(), snap.clone());

        let result = sym.resolve_file("lib.rs");
        for occ in &result.occurrences {
            assert_eq!(occ.graph_snapshot_id, snap.graph_snapshot_id);
            assert_eq!(occ.language, SymbolLanguage::Rust);
            assert!(!occ.content_hash.is_empty());
            assert!(!occ.structural_hash.is_empty());
            assert!(occ.line_start > 0);
            assert!(occ.line_end >= occ.line_start);
            assert!(occ.capture_kind.is_some());
            assert_eq!(occ.parser_version.as_deref(), Some(PARSER_VERSION));
        }
    }

    #[test]
    fn maps_capture_kinds_correctly() {
        let dir = setup_rust_fixture_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let snap = resolver.resolve_head().unwrap();
        let sym = RustSymbolResolver::new(dir.path(), snap);

        let result = sym.resolve_file("lib.rs");
        let by_name = |name: &str| -> &SymbolOccurrence {
            result
                .occurrences
                .iter()
                .find(|o| o.symbol_name == name)
                .unwrap()
        };

        assert_eq!(by_name("Config").symbol_kind, SymbolKind::Struct);
        assert_eq!(
            by_name("Config").capture_kind,
            Some(RustCaptureKind::DefinitionClass)
        );

        assert_eq!(by_name("Status").symbol_kind, SymbolKind::Enum);
        assert_eq!(
            by_name("Status").capture_kind,
            Some(RustCaptureKind::DefinitionClass)
        );

        assert_eq!(by_name("RawData").symbol_kind, SymbolKind::Union);
        assert_eq!(
            by_name("RawData").capture_kind,
            Some(RustCaptureKind::DefinitionClass)
        );

        assert_eq!(by_name("UserId").symbol_kind, SymbolKind::TypeAlias);
        assert_eq!(
            by_name("UserId").capture_kind,
            Some(RustCaptureKind::DefinitionClass)
        );

        assert_eq!(by_name("Validator").symbol_kind, SymbolKind::Trait);
        assert_eq!(
            by_name("Validator").capture_kind,
            Some(RustCaptureKind::DefinitionInterface)
        );

        assert_eq!(by_name("process").symbol_kind, SymbolKind::Function);
        assert_eq!(
            by_name("process").capture_kind,
            Some(RustCaptureKind::DefinitionFunction)
        );

        assert_eq!(by_name("utils").symbol_kind, SymbolKind::Module);
        assert_eq!(
            by_name("utils").capture_kind,
            Some(RustCaptureKind::DefinitionModule)
        );

        assert_eq!(by_name("my_macro").symbol_kind, SymbolKind::Macro);
        assert_eq!(
            by_name("my_macro").capture_kind,
            Some(RustCaptureKind::DefinitionMacro)
        );
    }

    #[test]
    fn methods_inside_impl_are_definition_method() {
        let dir = setup_rust_fixture_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let snap = resolver.resolve_head().unwrap();
        let sym = RustSymbolResolver::new(dir.path(), snap);

        let result = sym.resolve_file("lib.rs");
        let methods: Vec<&SymbolOccurrence> = result
            .occurrences
            .iter()
            .filter(|o| o.symbol_kind == SymbolKind::Method)
            .collect();

        assert!(!methods.is_empty(), "should find methods in impl blocks");
        for m in &methods {
            assert_eq!(m.capture_kind, Some(RustCaptureKind::DefinitionMethod));
        }

        let method_names: Vec<&str> = methods.iter().map(|m| m.symbol_name.as_str()).collect();
        assert!(
            method_names.contains(&"validate"),
            "should find validate method"
        );
        assert!(method_names.contains(&"new"), "should find new method");
    }

    #[test]
    fn impl_blocks_are_reference_implementation() {
        let dir = setup_rust_fixture_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let snap = resolver.resolve_head().unwrap();
        let sym = RustSymbolResolver::new(dir.path(), snap);

        let result = sym.resolve_file("lib.rs");
        let impls: Vec<&SymbolOccurrence> = result
            .occurrences
            .iter()
            .filter(|o| o.symbol_kind == SymbolKind::Impl)
            .collect();

        assert!(!impls.is_empty(), "should find impl blocks");
        for imp in &impls {
            assert_eq!(
                imp.capture_kind,
                Some(RustCaptureKind::ReferenceImplementation)
            );
        }
    }

    #[test]
    fn find_symbol_returns_single_verified() {
        let dir = setup_rust_fixture_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let snap = resolver.resolve_head().unwrap();
        let sym = RustSymbolResolver::new(dir.path(), snap);

        let result = sym.find_symbol("process");
        assert_eq!(result.evidence_state, GraphEvidenceState::Verified);
        assert_eq!(result.occurrences.len(), 1);
        assert_eq!(result.occurrences[0].symbol_name, "process");
    }

    #[test]
    fn find_symbol_not_found() {
        let dir = setup_rust_fixture_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let snap = resolver.resolve_head().unwrap();
        let sym = RustSymbolResolver::new(dir.path(), snap);

        let result = sym.find_symbol("nonexistent_function");
        assert_eq!(result.evidence_state, GraphEvidenceState::NotFound);
        assert!(result.occurrences.is_empty());
    }

    #[test]
    fn non_rs_file_returns_unsupported() {
        let dir = setup_rust_fixture_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let snap = resolver.resolve_head().unwrap();
        let sym = RustSymbolResolver::new(dir.path(), snap);

        let result = sym.resolve_file("readme.md");
        assert_eq!(result.evidence_state, GraphEvidenceState::Unsupported);
    }

    #[test]
    fn unsupported_language_returns_unsupported() {
        let dir = setup_rust_fixture_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let snap = resolver.resolve_head().unwrap();
        let sym = RustSymbolResolver::new(dir.path(), snap);

        let result = sym.unsupported_language("python");
        assert_eq!(result.evidence_state, GraphEvidenceState::Unsupported);
        assert_eq!(result.language, SymbolLanguage::Unsupported);
    }

    #[test]
    fn missing_file_returns_not_found() {
        let dir = setup_rust_fixture_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let snap = resolver.resolve_head().unwrap();
        let sym = RustSymbolResolver::new(dir.path(), snap);

        let result = sym.resolve_file("nonexistent.rs");
        assert_eq!(result.evidence_state, GraphEvidenceState::NotFound);
    }

    #[test]
    fn dirty_file_returns_unknown_stale() {
        let dir = setup_rust_fixture_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let snap = resolver.resolve_head().unwrap();

        // Modify committed file without committing.
        std::fs::write(dir.path().join("lib.rs"), "pub fn changed() {}\n").unwrap();

        let sym = RustSymbolResolver::new(dir.path(), snap);
        let result = sym.resolve_file("lib.rs");
        assert_eq!(result.evidence_state, GraphEvidenceState::UnknownStale);
    }

    #[test]
    fn structural_hash_changes_when_structure_changes() {
        let dir = setup_rust_fixture_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let snap = resolver.resolve_head().unwrap();
        let sym = RustSymbolResolver::new(dir.path(), snap.clone());

        let result1 = sym.resolve_file("lib.rs");
        let process_hash1 = result1
            .occurrences
            .iter()
            .find(|o| o.symbol_name == "process")
            .unwrap()
            .structural_hash
            .clone();

        // Change the function structure.
        std::fs::write(
            dir.path().join("lib.rs"),
            "pub fn process(input: &str) -> String {\n    let x = input.to_uppercase();\n    x\n}\n",
        )
        .unwrap();
        Command::new("git")
            .args(["add", "lib.rs"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "change structure"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let snap2 = resolver.resolve_head().unwrap();
        let sym2 = RustSymbolResolver::new(dir.path(), snap2);
        let result2 = sym2.resolve_file("lib.rs");
        let process_hash2 = result2
            .occurrences
            .iter()
            .find(|o| o.symbol_name == "process")
            .unwrap()
            .structural_hash
            .clone();

        assert_ne!(
            process_hash1, process_hash2,
            "structural hash should change when AST structure changes"
        );
    }

    #[test]
    fn moved_symbol_to_different_file_produces_not_found_in_old() {
        let dir = setup_rust_fixture_repo();

        // Move the process function to a new file.
        let new_content = "pub fn process(input: &str) -> String { input.to_uppercase() }\n";
        let old_content = std::fs::read_to_string(dir.path().join("lib.rs")).unwrap();
        let remaining = old_content.replace(
            "pub fn process(input: &str) -> String {\n    input.to_uppercase()\n}\n",
            "",
        );

        std::fs::write(dir.path().join("lib.rs"), remaining).unwrap();
        std::fs::write(dir.path().join("moved.rs"), new_content).unwrap();

        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "move process to moved.rs"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let resolver = SnapshotResolver::new(dir.path());
        let snap = resolver.resolve_head().unwrap();
        let sym = RustSymbolResolver::new(dir.path(), snap);

        // Old file should not contain "process" anymore.
        let old_result = sym.resolve_file("lib.rs");
        let old_names: Vec<&str> = old_result
            .occurrences
            .iter()
            .map(|o| o.symbol_name.as_str())
            .collect();
        assert!(
            !old_names.contains(&"process"),
            "process should not be in old file after move"
        );

        // New file should contain "process".
        let new_result = sym.resolve_file("moved.rs");
        assert_eq!(new_result.evidence_state, GraphEvidenceState::Verified);
        let new_names: Vec<&str> = new_result
            .occurrences
            .iter()
            .map(|o| o.symbol_name.as_str())
            .collect();
        assert!(
            new_names.contains(&"process"),
            "process should be in new file after move"
        );
    }
}
