//! Explicit dependency build graph with fine-grained edge types, cycle detection,
//! change propagation, and deterministic traversal.

use crate::fingerprint::ContentFingerprint;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::fmt;

/// Unique identifier for a build graph node.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NodeId(pub String);

impl NodeId {
    pub fn new(id: impl Into<String>) -> Self {
        NodeId(id.into())
    }

    pub fn workspace() -> Self {
        NodeId("ws:workspace".into())
    }

    pub fn source(path: &str) -> Self {
        NodeId(format!("src:{}", path))
    }

    pub fn module(name: &str) -> Self {
        NodeId(format!("mod:{}", name))
    }

    pub fn function(module: &str, name: &str) -> Self {
        NodeId(format!("fn:{}::{}", module, name))
    }

    pub fn type_interface(name: &str) -> Self {
        NodeId(format!("type:{}", name))
    }

    pub fn generic_template(name: &str) -> Self {
        NodeId(format!("tmpl:{}", name))
    }

    pub fn concrete_instantiation(name: &str, type_args: &[&str]) -> Self {
        NodeId(format!("inst:{}<{}>", name, type_args.join(",")))
    }

    pub fn codegen_fragment(symbol: &str, target: &str) -> Self {
        NodeId(format!("frag:{}:{}", target, symbol))
    }

    pub fn object_file(name: &str, target: &str) -> Self {
        NodeId(format!("obj:{}:{}", target, name))
    }

    pub fn archive(name: &str, target: &str) -> Self {
        NodeId(format!("arch:{}:{}", target, name))
    }

    pub fn executable(name: &str, target: &str) -> Self {
        NodeId(format!("exe:{}:{}", target, name))
    }

    pub fn runtime_artifact(name: &str, target: &str) -> Self {
        NodeId(format!("rt:{}:{}", target, name))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The kind and specific payload of a build graph node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeKind {
    Workspace,
    SourceFile {
        path: String,
        fingerprint: ContentFingerprint,
    },
    Module {
        name: String,
        file_id: u32,
        interface_fingerprint: ContentFingerprint,
    },
    Declaration {
        name: String,
        module: String,
        fingerprint: ContentFingerprint,
    },
    TypeInterface {
        name: String,
        layout_fingerprint: ContentFingerprint,
    },
    GenericTemplate {
        name: String,
        template_fingerprint: ContentFingerprint,
    },
    ConcreteInstantiation {
        name: String,
        type_args: Vec<String>,
        fingerprint: ContentFingerprint,
    },
    Function {
        name: String,
        module: String,
        body_fingerprint: ContentFingerprint,
        sig_fingerprint: ContentFingerprint,
    },
    CodegenFragment {
        symbol: String,
        target: String,
        fragment_fingerprint: ContentFingerprint,
    },
    ObjectFile {
        name: String,
        target: String,
        object_fingerprint: ContentFingerprint,
    },
    Archive {
        name: String,
        members: Vec<String>,
        archive_fingerprint: ContentFingerprint,
    },
    Executable {
        name: String,
        link_fingerprint: ContentFingerprint,
    },
    RuntimeArtifact {
        name: String,
        fingerprint: ContentFingerprint,
    },
}

/// The semantic strength and domain of a dependency edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DependencyKind {
    /// Inlining or internal execution requiring callee's body.
    Body,
    /// Calling convention or ABI requiring callee's signature.
    Signature,
    /// Memory layout (size, alignment, field offsets) of a struct/enum.
    TypeLayout,
    /// Trait method lookup, conformance, or vtable construction.
    TraitResolution,
    /// Concrete generic instantiation depending on template AST/MIR.
    GenericTemplate,
    /// Native machine code generation depending on MIR and target settings.
    Codegen,
    /// Executable creation depending on constituent object files, archives, and link options.
    Link,
    /// Source line mappings or debug tables depending on source files or flags.
    Debug,
}

/// A node within the dependency build graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildNode {
    pub id: NodeId,
    pub kind: NodeKind,
}

impl BuildNode {
    pub fn new(id: NodeId, kind: NodeKind) -> Self {
        Self { id, kind }
    }
}

/// Reason explaining why a node was marked invalid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvalidationReason {
    SourceChanged,
    BodyChanged,
    SignatureChanged,
    TypeLayoutChanged,
    TraitResolutionChanged,
    GenericTemplateChanged,
    CodegenConfigChanged,
    LinkConfigChanged,
    DependencyInvalidated(NodeId, DependencyKind),
    CacheCorrupted(String),
}

impl fmt::Display for InvalidationReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InvalidationReason::SourceChanged => write!(f, "source file content changed"),
            InvalidationReason::BodyChanged => write!(f, "function body changed"),
            InvalidationReason::SignatureChanged => write!(f, "declaration signature changed"),
            InvalidationReason::TypeLayoutChanged => {
                write!(f, "type layout or field alignment changed")
            }
            InvalidationReason::TraitResolutionChanged => write!(f, "trait implementation changed"),
            InvalidationReason::GenericTemplateChanged => {
                write!(f, "generic template definition changed")
            }
            InvalidationReason::CodegenConfigChanged => {
                write!(f, "codegen configuration or target changed")
            }
            InvalidationReason::LinkConfigChanged => write!(f, "link inputs or flags changed"),
            InvalidationReason::DependencyInvalidated(id, kind) => {
                write!(f, "dependency '{}' changed ({:?})", id, kind)
            }
            InvalidationReason::CacheCorrupted(msg) => write!(f, "cache corrupted: {}", msg),
        }
    }
}

/// Explicit dependency graph with deterministic operations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BuildGraph {
    nodes: BTreeMap<NodeId, BuildNode>,
    /// from (consumer) -> to (providers) with dependency kind
    dependencies: BTreeMap<NodeId, BTreeMap<NodeId, DependencyKind>>,
    /// from (provider) -> to (consumers) with dependency kind
    dependents: BTreeMap<NodeId, BTreeMap<NodeId, DependencyKind>>,
}

impl BuildGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn contains_node(&self, id: &NodeId) -> bool {
        self.nodes.contains_key(id)
    }

    pub fn get_node(&self, id: &NodeId) -> Option<&BuildNode> {
        self.nodes.get(id)
    }

    pub fn get_node_mut(&mut self, id: &NodeId) -> Option<&mut BuildNode> {
        self.nodes.get_mut(id)
    }

    pub fn add_node(&mut self, node: BuildNode) {
        let id = node.id.clone();
        self.nodes.insert(id, node);
    }

    /// Add a directed dependency edge: `consumer` depends on `provider` with `kind`.
    pub fn add_dependency(&mut self, consumer: NodeId, provider: NodeId, kind: DependencyKind) {
        self.dependencies
            .entry(consumer.clone())
            .or_default()
            .insert(provider.clone(), kind);
        self.dependents
            .entry(provider)
            .or_default()
            .insert(consumer, kind);
    }

    pub fn remove_node(&mut self, id: &NodeId) -> Option<BuildNode> {
        if let Some(deps) = self.dependencies.remove(id) {
            for (provider, _) in deps {
                if let Some(rev) = self.dependents.get_mut(&provider) {
                    rev.remove(id);
                }
            }
        }
        if let Some(revs) = self.dependents.remove(id) {
            for (consumer, _) in revs {
                if let Some(deps) = self.dependencies.get_mut(&consumer) {
                    deps.remove(id);
                }
            }
        }
        self.nodes.remove(id)
    }

    /// Providers that `consumer` depends on.
    pub fn get_dependencies(&self, consumer: &NodeId) -> Vec<(&NodeId, DependencyKind)> {
        self.dependencies
            .get(consumer)
            .map(|deps| deps.iter().map(|(id, k)| (id, *k)).collect())
            .unwrap_or_default()
    }

    /// Consumers that depend on `provider` (reverse dependencies).
    pub fn get_dependents(&self, provider: &NodeId) -> Vec<(&NodeId, DependencyKind)> {
        self.dependents
            .get(provider)
            .map(|deps| deps.iter().map(|(id, k)| (id, *k)).collect())
            .unwrap_or_default()
    }

    /// Detect illegal cyclic dependencies (ignoring allowed recursive function calls).
    pub fn detect_cycles(&self) -> Result<(), Vec<NodeId>> {
        let mut visited = HashSet::new();
        let mut on_stack = HashSet::new();
        let mut cycle_path = Vec::new();

        for node_id in self.nodes.keys() {
            if !visited.contains(node_id) {
                if self.find_cycle(node_id, &mut visited, &mut on_stack, &mut cycle_path) {
                    return Err(cycle_path);
                }
            }
        }
        Ok(())
    }

    fn find_cycle(
        &self,
        curr: &NodeId,
        visited: &mut HashSet<NodeId>,
        on_stack: &mut HashSet<NodeId>,
        cycle_path: &mut Vec<NodeId>,
    ) -> bool {
        visited.insert(curr.clone());
        on_stack.insert(curr.clone());
        cycle_path.push(curr.clone());

        if let Some(deps) = self.dependencies.get(curr) {
            for (dep_id, kind) in deps {
                // Recursive function calls are safe and allowed
                let is_func_call =
                    *kind == DependencyKind::Body || *kind == DependencyKind::Signature;
                let is_func_to_func = curr.0.starts_with("fn:") && dep_id.0.starts_with("fn:");
                if is_func_call && is_func_to_func {
                    continue;
                }

                if !visited.contains(dep_id) {
                    if self.find_cycle(dep_id, visited, on_stack, cycle_path) {
                        return true;
                    }
                } else if on_stack.contains(dep_id) {
                    cycle_path.push(dep_id.clone());
                    return true;
                }
            }
        }

        on_stack.remove(curr);
        cycle_path.pop();
        false
    }

    /// Propagate invalidations transitively following dependency semantics.
    pub fn invalidate(
        &self,
        roots: &[(NodeId, InvalidationReason)],
    ) -> BTreeMap<NodeId, InvalidationReason> {
        let mut invalidated = BTreeMap::new();
        let mut queue = VecDeque::new();

        for (id, reason) in roots {
            if self.nodes.contains_key(id) {
                invalidated.insert(id.clone(), reason.clone());
                queue.push_back((id.clone(), reason.clone()));
            }
        }

        while let Some((changed_id, reason)) = queue.pop_front() {
            if let Some(consumers) = self.dependents.get(&changed_id) {
                for (consumer_id, edge_kind) in consumers {
                    if invalidated.contains_key(consumer_id) {
                        continue;
                    }

                    // Check if edge kind demands invalidation for this change reason
                    let should_invalidate = match (&reason, edge_kind) {
                        // Function body change: does not invalidate callers that only care about signature
                        (InvalidationReason::BodyChanged, DependencyKind::Signature) => false,
                        // Function body change: does invalidate inlined callers, containing codegen fragment, object, executable
                        (InvalidationReason::BodyChanged, DependencyKind::Body) => true,
                        (InvalidationReason::BodyChanged, DependencyKind::Codegen) => true,
                        (InvalidationReason::BodyChanged, DependencyKind::Link) => true,
                        // Signature change: invalidates everything depending on signature or body
                        (InvalidationReason::SignatureChanged, _) => true,
                        // Type layout change: invalidates consumers of that layout and dependents
                        (InvalidationReason::TypeLayoutChanged, DependencyKind::TypeLayout) => true,
                        (InvalidationReason::TypeLayoutChanged, DependencyKind::Codegen) => true,
                        (InvalidationReason::TypeLayoutChanged, DependencyKind::Link) => true,
                        (InvalidationReason::TypeLayoutChanged, _) => true,
                        // Default transitive propagation for other reasons
                        _ => true,
                    };

                    if should_invalidate {
                        let dep_reason = InvalidationReason::DependencyInvalidated(
                            changed_id.clone(),
                            *edge_kind,
                        );
                        invalidated.insert(consumer_id.clone(), dep_reason.clone());
                        queue.push_back((consumer_id.clone(), dep_reason));
                    }
                }
            }
        }

        invalidated
    }

    /// Remove nodes that are no longer active in the workspace and prune dangling edges.
    pub fn clean_stale_nodes(&mut self, active_nodes: &HashSet<NodeId>) -> Vec<NodeId> {
        let stale: Vec<NodeId> = self
            .nodes
            .keys()
            .filter(|id| !active_nodes.contains(id))
            .cloned()
            .collect();

        for id in &stale {
            self.remove_node(id);
        }
        stale
    }

    /// Return nodes in deterministic topological order (dependencies before dependents).
    pub fn topological_order(&self) -> Result<Vec<NodeId>, Vec<NodeId>> {
        let mut in_degree: BTreeMap<NodeId, usize> = BTreeMap::new();
        for id in self.nodes.keys() {
            in_degree.insert(id.clone(), 0);
        }

        for (consumer, deps) in &self.dependencies {
            let count = deps
                .iter()
                .filter(|(dep, kind)| {
                    // Filter out recursive function self/cycle edges
                    let is_func = consumer.0.starts_with("fn:") && dep.0.starts_with("fn:");
                    let is_rec =
                        **kind == DependencyKind::Body || **kind == DependencyKind::Signature;
                    !(is_func && is_rec)
                })
                .count();
            in_degree.insert(consumer.clone(), count);
        }

        let mut queue: BTreeSet<NodeId> = BTreeSet::new();
        for (id, &deg) in &in_degree {
            if deg == 0 {
                queue.insert(id.clone());
            }
        }

        let mut order = Vec::with_capacity(self.nodes.len());
        while let Some(id) = queue.pop_first() {
            order.push(id.clone());

            if let Some(consumers) = self.dependents.get(&id) {
                for (consumer, kind) in consumers {
                    let is_func = consumer.0.starts_with("fn:") && id.0.starts_with("fn:");
                    let is_rec =
                        *kind == DependencyKind::Body || *kind == DependencyKind::Signature;
                    if is_func && is_rec {
                        continue;
                    }

                    if let Some(deg) = in_degree.get_mut(consumer) {
                        if *deg > 0 {
                            *deg -= 1;
                            if *deg == 0 {
                                queue.insert(consumer.clone());
                            }
                        }
                    }
                }
            }
        }

        if order.len() == self.nodes.len() {
            Ok(order)
        } else {
            let remaining: Vec<NodeId> = self
                .nodes
                .keys()
                .filter(|id| !order.contains(id))
                .cloned()
                .collect();
            Err(remaining)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graph_add_and_reverse_lookup() {
        let mut graph = BuildGraph::new();
        let src = BuildNode::new(
            NodeId::source("main.va"),
            NodeKind::SourceFile {
                path: "main.va".into(),
                fingerprint: ContentFingerprint::ZERO,
            },
        );
        let obj = BuildNode::new(
            NodeId::object_file("main.o", "x86_64-linux"),
            NodeKind::ObjectFile {
                name: "main.o".into(),
                target: "x86_64-linux".into(),
                object_fingerprint: ContentFingerprint::ZERO,
            },
        );
        graph.add_node(src);
        graph.add_node(obj);

        graph.add_dependency(
            NodeId::object_file("main.o", "x86_64-linux"),
            NodeId::source("main.va"),
            DependencyKind::Codegen,
        );

        let deps = graph.get_dependencies(&NodeId::object_file("main.o", "x86_64-linux"));
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].0, &NodeId::source("main.va"));

        let rev = graph.get_dependents(&NodeId::source("main.va"));
        assert_eq!(rev.len(), 1);
        assert_eq!(rev[0].0, &NodeId::object_file("main.o", "x86_64-linux"));
    }

    #[test]
    fn test_fine_grained_function_invalidation() {
        let mut graph = BuildGraph::new();
        let fn_a = NodeId::function("main", "compute");
        let fn_b = NodeId::function("main", "caller");
        let obj = NodeId::object_file("main.o", "x86_64-linux");

        graph.add_node(BuildNode::new(
            fn_a.clone(),
            NodeKind::Function {
                name: "compute".into(),
                module: "main".into(),
                body_fingerprint: ContentFingerprint::ZERO,
                sig_fingerprint: ContentFingerprint::ZERO,
            },
        ));
        graph.add_node(BuildNode::new(
            fn_b.clone(),
            NodeKind::Function {
                name: "caller".into(),
                module: "main".into(),
                body_fingerprint: ContentFingerprint::ZERO,
                sig_fingerprint: ContentFingerprint::ZERO,
            },
        ));
        graph.add_node(BuildNode::new(
            obj.clone(),
            NodeKind::ObjectFile {
                name: "main.o".into(),
                target: "x86_64-linux".into(),
                object_fingerprint: ContentFingerprint::ZERO,
            },
        ));

        // caller only depends on signature of compute
        graph.add_dependency(fn_b.clone(), fn_a.clone(), DependencyKind::Signature);
        // obj depends on codegen of compute and caller
        graph.add_dependency(obj.clone(), fn_a.clone(), DependencyKind::Codegen);
        graph.add_dependency(obj.clone(), fn_b.clone(), DependencyKind::Codegen);

        // Body change of compute: should invalidate compute and obj, but NOT caller
        let inv = graph.invalidate(&[(fn_a.clone(), InvalidationReason::BodyChanged)]);
        assert!(inv.contains_key(&fn_a));
        assert!(inv.contains_key(&obj));
        assert!(
            !inv.contains_key(&fn_b),
            "caller should not be invalidated by body-only change"
        );

        // Signature change of compute: MUST invalidate caller
        let inv_sig = graph.invalidate(&[(fn_a.clone(), InvalidationReason::SignatureChanged)]);
        assert!(inv_sig.contains_key(&fn_a));
        assert!(
            inv_sig.contains_key(&fn_b),
            "caller must be invalidated by signature change"
        );
        assert!(inv_sig.contains_key(&obj));
    }

    #[test]
    fn test_recursive_function_not_flagged_as_illegal_cycle() {
        let mut graph = BuildGraph::new();
        let fn_fac = NodeId::function("main", "factorial");
        graph.add_node(BuildNode::new(
            fn_fac.clone(),
            NodeKind::Function {
                name: "factorial".into(),
                module: "main".into(),
                body_fingerprint: ContentFingerprint::ZERO,
                sig_fingerprint: ContentFingerprint::ZERO,
            },
        ));
        // factorial calls itself
        graph.add_dependency(fn_fac.clone(), fn_fac.clone(), DependencyKind::Body);

        assert!(graph.detect_cycles().is_ok());
    }

    #[test]
    fn test_topological_sort_deterministic() {
        let mut graph = BuildGraph::new();
        let src = NodeId::source("main.va");
        let obj = NodeId::object_file("main.o", "target");
        let exe = NodeId::executable("app", "target");

        graph.add_node(BuildNode::new(src.clone(), NodeKind::Workspace));
        graph.add_node(BuildNode::new(obj.clone(), NodeKind::Workspace));
        graph.add_node(BuildNode::new(exe.clone(), NodeKind::Workspace));

        graph.add_dependency(obj.clone(), src.clone(), DependencyKind::Codegen);
        graph.add_dependency(exe.clone(), obj.clone(), DependencyKind::Link);

        let order = graph.topological_order().expect("topological sort");
        assert_eq!(order, vec![src, obj, exe]);
    }
}
