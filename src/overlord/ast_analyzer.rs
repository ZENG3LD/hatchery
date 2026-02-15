//! AST-based code quality analyzer for Rust source files.
//!
//! Analyzes function implementations to detect stub patterns, unused parameters,
//! and compute quality scores based on complexity metrics.

use std::collections::HashSet;
use syn::visit::{self, Visit};

// ============================================================================
// Public Types
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub struct AstQualityReport {
    pub functions: Vec<FunctionQuality>,
    pub overall_score: f64,
    pub total_functions: usize,
    pub suspicious_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionQuality {
    pub name: String,
    pub line: usize,
    pub is_method: bool,
    pub score: f64,
    pub metrics: FunctionMetrics,
    pub issues: Vec<QualityIssue>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionMetrics {
    pub param_count: usize,
    pub params_used: usize,
    pub uses_self: bool,
    pub statement_count: usize,
    pub expression_count: usize,
    pub has_control_flow: bool,
    pub function_call_count: usize,
    pub has_computation: bool,
    pub local_binding_count: usize,
    pub returns_literal: bool,
    pub returns_default: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum QualityIssue {
    UnusedParams { unused: Vec<String> },
    ReturnsLiteral,
    ReturnsDefault,
    EmptyBody,
    NoComputation,
    UnusedSelf,
}

// ============================================================================
// Main Entry Point
// ============================================================================

/// Analyze a Rust source file for function quality.
/// Returns None if parsing fails.
pub fn analyze_file(source: &str) -> Option<AstQualityReport> {
    let ast = syn::parse_file(source).ok()?;

    let mut functions = Vec::new();

    // Walk top-level items
    for item in &ast.items {
        match item {
            syn::Item::Fn(item_fn) => {
                if let Some(quality) = analyze_function(&item_fn.sig, &item_fn.block, false) {
                    functions.push(quality);
                }
            }
            syn::Item::Impl(item_impl) => {
                // Walk methods in impl blocks
                for impl_item in &item_impl.items {
                    if let syn::ImplItem::Fn(method) = impl_item {
                        if let Some(quality) = analyze_function(&method.sig, &method.block, true) {
                            functions.push(quality);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Compute overall metrics
    let total_functions = functions.len();
    let overall_score = if total_functions > 0 {
        functions.iter().map(|f| f.score).sum::<f64>() / total_functions as f64
    } else {
        0.0
    };

    let suspicious_count = functions.iter().filter(|f| f.score < 0.3).count();

    Some(AstQualityReport {
        functions,
        overall_score,
        total_functions,
        suspicious_count,
    })
}

// ============================================================================
// Function Analysis
// ============================================================================

fn analyze_function(sig: &syn::Signature, block: &syn::Block, is_method: bool) -> Option<FunctionQuality> {
    let name = sig.ident.to_string();
    let line = 0; // syn doesn't easily give us line numbers without Span::start()

    // Extract parameter names (excluding self)
    let param_names = extract_param_names(sig);
    let param_count = param_names.len();

    // Visit the function body to collect metrics
    let mut visitor = MetricsVisitor::new(param_names);
    visitor.visit_block(block);

    // Check return patterns
    let (returns_literal, returns_default) = analyze_return_pattern(block);

    // Filter out wildcard-discarded params from params_used
    let truly_used: HashSet<_> = visitor.params_used
        .difference(&visitor.wildcard_discarded)
        .cloned()
        .collect();

    // Build metrics
    let metrics = FunctionMetrics {
        param_count,
        params_used: truly_used.len(),
        uses_self: visitor.uses_self,
        statement_count: visitor.statement_count,
        expression_count: visitor.expression_count,
        has_control_flow: visitor.has_control_flow,
        function_call_count: visitor.function_call_count,
        has_computation: visitor.has_computation,
        local_binding_count: visitor.local_binding_count,
        returns_literal,
        returns_default,
    };

    // Compute score
    let score = compute_score(&metrics, is_method);

    // Detect issues
    let issues = detect_issues(&metrics, is_method, &visitor.param_names, &truly_used);

    Some(FunctionQuality {
        name,
        line,
        is_method,
        score,
        metrics,
        issues,
    })
}

// ============================================================================
// Scoring
// ============================================================================

fn compute_score(metrics: &FunctionMetrics, is_method: bool) -> f64 {
    // Instant zero: returns only a literal
    if metrics.returns_literal && metrics.statement_count <= 1 {
        return 0.0;
    }

    // Near zero: returns Default::default()
    if metrics.returns_default && metrics.statement_count <= 1 {
        return 0.1;
    }

    // Empty body
    if metrics.statement_count == 0 && metrics.expression_count == 0 {
        return 0.0;
    }

    let mut score = 0.0;

    // Param usage (30%)
    if metrics.param_count > 0 {
        let ratio = metrics.params_used as f64 / metrics.param_count as f64;
        score += ratio * 0.3;
    } else {
        score += 0.3; // No params = no penalty
    }

    // Body complexity (25%) — normalized to 5 statements
    let complexity = (metrics.statement_count as f64 / 5.0).min(1.0);
    score += complexity * 0.25;

    // Computation (20%)
    if metrics.has_computation || metrics.function_call_count > 0 {
        score += 0.2;
    }

    // Control flow (15%)
    if metrics.has_control_flow {
        score += 0.15;
    }

    // Local bindings (10%)
    let bindings = (metrics.local_binding_count as f64 / 3.0).min(1.0);
    score += bindings * 0.1;

    // Penalty: method that doesn't use self
    if is_method && !metrics.uses_self {
        score *= 0.5;
    }

    score
}

// ============================================================================
// Issue Detection
// ============================================================================

fn detect_issues(
    metrics: &FunctionMetrics,
    is_method: bool,
    param_names: &HashSet<String>,
    params_used: &HashSet<String>,
) -> Vec<QualityIssue> {
    let mut issues = Vec::new();

    // Unused parameters
    if metrics.param_count > 0 && metrics.params_used < metrics.param_count {
        let mut unused: Vec<String> = param_names
            .difference(params_used)
            .map(|s| s.clone())
            .collect();
        unused.sort();
        issues.push(QualityIssue::UnusedParams { unused });
    }

    if metrics.returns_literal {
        issues.push(QualityIssue::ReturnsLiteral);
    }

    if metrics.returns_default {
        issues.push(QualityIssue::ReturnsDefault);
    }

    if metrics.statement_count == 0 {
        issues.push(QualityIssue::EmptyBody);
    }

    if !metrics.has_computation && metrics.function_call_count == 0 {
        issues.push(QualityIssue::NoComputation);
    }

    if is_method && !metrics.uses_self {
        issues.push(QualityIssue::UnusedSelf);
    }

    issues
}

// ============================================================================
// Parameter Extraction
// ============================================================================

fn extract_param_names(sig: &syn::Signature) -> HashSet<String> {
    let mut names = HashSet::new();
    for input in &sig.inputs {
        match input {
            syn::FnArg::Typed(pat_type) => {
                extract_pat_names(&pat_type.pat, &mut names);
            }
            syn::FnArg::Receiver(_) => {} // self — handled separately
        }
    }
    names
}

fn extract_pat_names(pat: &syn::Pat, names: &mut HashSet<String>) {
    match pat {
        syn::Pat::Ident(ident) => {
            // Skip wildcard patterns like `_`
            let name = ident.ident.to_string();
            if !name.starts_with('_') {
                names.insert(name);
            }
        }
        syn::Pat::Tuple(tuple) => {
            for p in &tuple.elems {
                extract_pat_names(p, names);
            }
        }
        syn::Pat::TupleStruct(ts) => {
            for p in &ts.elems {
                extract_pat_names(p, names);
            }
        }
        syn::Pat::Struct(s) => {
            for field in &s.fields {
                extract_pat_names(&field.pat, names);
            }
        }
        syn::Pat::Reference(r) => {
            extract_pat_names(&r.pat, names);
        }
        _ => {}
    }
}

// ============================================================================
// Return Pattern Analysis
// ============================================================================

fn analyze_return_pattern(block: &syn::Block) -> (bool, bool) {
    let last_expr = if let Some(last_stmt) = block.stmts.last() {
        match last_stmt {
            syn::Stmt::Expr(expr, _) => Some(expr),
            _ => None,
        }
    } else {
        None
    };

    if let Some(expr) = last_expr {
        let returns_literal = matches!(expr, syn::Expr::Lit(_));
        let returns_default = is_default_expr(expr);
        (returns_literal, returns_default)
    } else {
        (false, false)
    }
}

fn is_default_expr(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Call(call) => {
            // Check for Default::default(), Vec::new(), String::new(), etc.
            if let syn::Expr::Path(path) = &*call.func {
                let path_str = path_to_string(&path.path);
                return path_str.ends_with("::default")
                    || path_str.ends_with("::new")
                    || path_str == "None"
                    || path_str == "Ok";
            }
            false
        }
        syn::Expr::Path(path) => {
            let path_str = path_to_string(&path.path);
            path_str == "None"
        }
        _ => false,
    }
}

fn path_to_string(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

// ============================================================================
// Metrics Visitor
// ============================================================================

struct MetricsVisitor {
    param_names: HashSet<String>,
    params_used: HashSet<String>,
    statement_count: usize,
    expression_count: usize,
    has_control_flow: bool,
    function_call_count: usize,
    has_computation: bool,
    local_binding_count: usize,
    uses_self: bool,
    /// Track params that appear in wildcard assignments like `let _ = param;`
    wildcard_discarded: HashSet<String>,
}

impl MetricsVisitor {
    fn new(param_names: HashSet<String>) -> Self {
        Self {
            param_names,
            params_used: HashSet::new(),
            statement_count: 0,
            expression_count: 0,
            has_control_flow: false,
            function_call_count: 0,
            has_computation: false,
            local_binding_count: 0,
            uses_self: false,
            wildcard_discarded: HashSet::new(),
        }
    }

    /// Check if an expression is a simple parameter reference (no computation)
    fn is_simple_param_ref(&self, expr: &syn::Expr) -> Option<String> {
        if let syn::Expr::Path(path) = expr {
            if let Some(ident) = path.path.get_ident() {
                let ident_str = ident.to_string();
                if self.param_names.contains(&ident_str) {
                    return Some(ident_str);
                }
            }
        }
        None
    }
}

impl<'ast> Visit<'ast> for MetricsVisitor {
    fn visit_stmt(&mut self, stmt: &'ast syn::Stmt) {
        self.statement_count += 1;

        if let syn::Stmt::Local(local) = stmt {
            // Check if this is a wildcard assignment like `let _ = param;`
            let is_wildcard = matches!(&local.pat, syn::Pat::Wild(_));

            if is_wildcard {
                // Check if the initializer is a simple parameter reference
                if let Some(init) = &local.init {
                    if let Some(param_name) = self.is_simple_param_ref(&init.expr) {
                        self.wildcard_discarded.insert(param_name);
                    }
                }
            } else {
                self.local_binding_count += 1;
            }
        }

        visit::visit_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &'ast syn::Expr) {
        self.expression_count += 1;

        match expr {
            // Control flow
            syn::Expr::If(_) | syn::Expr::Match(_) | syn::Expr::Loop(_)
            | syn::Expr::While(_) | syn::Expr::ForLoop(_) => {
                self.has_control_flow = true;
            }

            // Function/method calls
            syn::Expr::Call(_) | syn::Expr::MethodCall(_) => {
                self.function_call_count += 1;
            }

            // Computation
            syn::Expr::Binary(_) | syn::Expr::Unary(_) => {
                self.has_computation = true;
            }

            // Path expressions — check for param usage or self
            syn::Expr::Path(path) => {
                if let Some(ident) = path.path.get_ident() {
                    let ident_str = ident.to_string();
                    if ident_str == "self" {
                        self.uses_self = true;
                    } else if self.param_names.contains(&ident_str) {
                        self.params_used.insert(ident_str);
                    }
                }
            }

            // Field access on self
            syn::Expr::Field(field) => {
                if let syn::Expr::Path(base) = &*field.base {
                    if let Some(ident) = base.path.get_ident() {
                        if ident == "self" {
                            self.uses_self = true;
                        }
                    }
                }
            }

            // Macro invocations (e.g., todo!())
            syn::Expr::Macro(_) => {
                self.function_call_count += 1;
            }

            _ => {}
        }

        visit::visit_expr(self, expr);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stub_returns_zero() {
        let source = r#"
            fn stub() -> u32 { 0 }
        "#;
        let report = analyze_file(source).unwrap();
        assert_eq!(report.total_functions, 1);
        let func = &report.functions[0];
        assert_eq!(func.name, "stub");
        assert!(func.metrics.returns_literal);
        assert!(func.score < 0.05); // Should be ~0.0
        assert!(func.issues.contains(&QualityIssue::ReturnsLiteral));
    }

    #[test]
    fn test_stub_returns_default() {
        let source = r#"
            fn stub() -> Vec<u8> { Default::default() }
        "#;
        let report = analyze_file(source).unwrap();
        assert_eq!(report.total_functions, 1);
        let func = &report.functions[0];
        assert!(func.metrics.returns_default);
        assert!((func.score - 0.1).abs() < 0.05); // Should be ~0.1
        assert!(func.issues.contains(&QualityIssue::ReturnsDefault));
    }

    #[test]
    fn test_stub_unused_params() {
        let source = r#"
            fn stub(x: u32, y: &str) -> bool { false }
        "#;
        let report = analyze_file(source).unwrap();
        let func = &report.functions[0];
        assert_eq!(func.metrics.param_count, 2);
        assert_eq!(func.metrics.params_used, 0);

        let has_unused = func.issues.iter().any(|issue| {
            matches!(issue, QualityIssue::UnusedParams { unused } if unused.len() == 2)
        });
        assert!(has_unused);
    }

    #[test]
    fn test_real_function_high_score() {
        let source = r#"
            fn compute(x: u32, y: u32) -> u32 {
                let sum = x + y;
                if sum > 100 {
                    sum * 2
                } else {
                    sum
                }
            }
        "#;
        let report = analyze_file(source).unwrap();
        let func = &report.functions[0];
        assert_eq!(func.metrics.param_count, 2);
        assert_eq!(func.metrics.params_used, 2);
        assert!(func.metrics.has_control_flow);
        assert!(func.metrics.has_computation);
        assert!(func.score > 0.7);
    }

    #[test]
    fn test_empty_body() {
        let source = r#"
            fn noop() {}
        "#;
        let report = analyze_file(source).unwrap();
        let func = &report.functions[0];
        assert_eq!(func.metrics.statement_count, 0);
        assert!(func.issues.contains(&QualityIssue::EmptyBody));
        assert_eq!(func.score, 0.0);
    }

    #[test]
    fn test_method_uses_self() {
        let source = r#"
            struct Foo { value: u32 }
            impl Foo {
                fn bar(&self) -> u32 { self.value }
            }
        "#;
        let report = analyze_file(source).unwrap();
        let func = &report.functions[0];
        assert_eq!(func.name, "bar");
        assert!(func.is_method);
        assert!(func.metrics.uses_self);
        assert!(!func.issues.contains(&QualityIssue::UnusedSelf));
    }

    #[test]
    fn test_method_unused_self() {
        let source = r#"
            struct Foo { value: u32 }
            impl Foo {
                fn bar(&self) -> u32 { 42 }
            }
        "#;
        let report = analyze_file(source).unwrap();
        let func = &report.functions[0];
        assert!(func.is_method);
        assert!(!func.metrics.uses_self);
        assert!(func.issues.contains(&QualityIssue::UnusedSelf));
        // Score should be halved due to unused self
        assert!(func.score < 0.3);
    }

    #[test]
    fn test_complex_function() {
        let source = r#"
            fn process(data: &[u8], threshold: usize) -> Vec<u8> {
                let mut result = Vec::new();
                let mut sum = 0;

                for &byte in data {
                    sum += byte as usize;
                    if sum > threshold {
                        result.push((sum % 256) as u8);
                        sum = 0;
                    }
                }

                result
            }
        "#;
        let report = analyze_file(source).unwrap();
        let func = &report.functions[0];
        assert_eq!(func.metrics.param_count, 2);
        assert_eq!(func.metrics.params_used, 2);
        assert!(func.metrics.has_control_flow);
        assert!(func.metrics.has_computation);
        assert!(func.metrics.function_call_count > 0);
        assert!(func.metrics.local_binding_count >= 2);
        assert!(func.score > 0.85);
    }

    #[test]
    fn test_formal_compliance() {
        let source = r#"
            fn calc(a: f64, b: f64) -> f64 {
                let _ = a;
                let _ = b;
                0.0
            }
        "#;
        let report = analyze_file(source).unwrap();
        let func = &report.functions[0];
        // Wildcard patterns don't count as real usage
        assert_eq!(func.metrics.params_used, 0);
        assert!(func.issues.contains(&QualityIssue::NoComputation));
        assert!(func.score < 0.3);
    }

    #[test]
    fn test_simple_getter_acceptable() {
        let source = r#"
            struct Person { name: String }
            impl Person {
                fn name(&self) -> &str {
                    &self.name
                }
            }
        "#;
        let report = analyze_file(source).unwrap();
        let func = &report.functions[0];
        assert!(func.is_method);
        assert!(func.metrics.uses_self);
        // Getters are valid — should have reasonable score
        assert!(func.score > 0.3);
    }

    #[test]
    fn test_wrapper_function() {
        let source = r#"
            fn add(a: u32, b: u32) -> u32 { a + b }
        "#;
        let report = analyze_file(source).unwrap();
        let func = &report.functions[0];
        assert_eq!(func.metrics.params_used, 2);
        assert!(func.metrics.has_computation);
        assert!(func.score > 0.4);
    }

    #[test]
    fn test_multiple_functions_report() {
        let source = r#"
            fn stub() -> u32 { 0 }

            fn real_work(x: u32) -> u32 {
                let y = x * 2;
                if y > 10 { y } else { x }
            }

            fn mixed(a: u32, b: u32) -> u32 {
                a // b unused
            }
        "#;
        let report = analyze_file(source).unwrap();
        assert_eq!(report.total_functions, 3);

        // stub should have low score
        let stub = report.functions.iter().find(|f| f.name == "stub").unwrap();
        assert!(stub.score < 0.1);

        // real_work should have high score
        let real = report.functions.iter().find(|f| f.name == "real_work").unwrap();
        assert!(real.score > 0.6);

        // mixed should have low-medium score (uses only 1/2 params, no computation)
        let mixed = report.functions.iter().find(|f| f.name == "mixed").unwrap();
        assert!(mixed.score > 0.1 && mixed.score < 0.4);
        assert!(mixed.issues.iter().any(|i| matches!(i, QualityIssue::UnusedParams { .. })));

        // Both stub and mixed are suspicious (score < 0.3)
        assert_eq!(report.suspicious_count, 2);
    }

    #[test]
    fn test_parse_failure_returns_none() {
        let source = "this is not valid Rust code {{{";
        let report = analyze_file(source);
        assert!(report.is_none());
    }

    #[test]
    fn test_analyze_real_hatchery_code() {
        // Read a real file from hatchery source
        let source = include_str!("parsers.rs");
        let report = analyze_file(source).unwrap();

        // parsers.rs has many real functions — should score well
        assert!(report.total_functions >= 5, "Expected >=5 functions in parsers.rs, got {}", report.total_functions);
        assert!(report.overall_score > 0.5, "Expected overall score > 0.5 for parsers.rs, got {:.2}", report.overall_score);
        assert!(report.suspicious_count < report.total_functions / 2,
            "Too many suspicious functions in parsers.rs: {}/{}", report.suspicious_count, report.total_functions);

        // Print detailed report for manual inspection
        for func in &report.functions {
            eprintln!("  {}: score={:.2}, params={}/{}, stmts={}, calls={}, issues={:?}",
                func.name, func.score, func.metrics.params_used, func.metrics.param_count,
                func.metrics.statement_count, func.metrics.function_call_count, func.issues);
        }
    }

    #[test]
    fn test_analyze_real_code_checks() {
        let source = include_str!("code_checks.rs");
        let report = analyze_file(source).unwrap();

        assert!(report.total_functions > 3);
        assert!(report.overall_score > 0.4);

        for func in &report.functions {
            eprintln!("  {}: score={:.2}, issues={:?}", func.name, func.score, func.issues);
        }
    }

    #[test]
    fn test_todo_macro_detected() {
        let source = r#"
            fn stub() { todo!() }
        "#;
        let report = analyze_file(source).unwrap();
        let func = &report.functions[0];
        // todo!() should be counted as a function call (macro invocation)
        assert!(func.metrics.function_call_count > 0);
    }
}
