#![deny(unsafe_code)]

use crate::node::Node;
use std::fmt;

/// Unique identifier for a node within a DOM arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub usize);

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NodeId({})", self.0)
    }
}

/// An index-backed contiguous arena for DOM nodes.
///
/// This design guarantees cache locality, fast traversals, and zero
/// runtime reference counting cycles.
#[derive(Debug, Clone, Default)]
pub struct Arena {
    nodes: Vec<Node>,
}

impl Arena {
    /// Creates a new empty arena.
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    /// Allocates a new node in the arena using a closure that receives the newly minted `NodeId`.
    pub fn alloc(&mut self, create: impl FnOnce(NodeId) -> Node) -> NodeId {
        let id = NodeId(self.nodes.len());
        let node = create(id);
        self.nodes.push(node);
        id
    }

    /// Retrieves an immutable reference to the node with the given `NodeId`.
    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id.0)
    }

    /// Retrieves a mutable reference to the node with the given `NodeId`.
    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(id.0)
    }

    /// Checks if a node with the given `NodeId` exists in the arena.
    pub fn contains(&self, id: NodeId) -> bool {
        id.0 < self.nodes.len()
    }

    /// Returns the total number of nodes in the arena.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns true if the arena contains no nodes.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Returns an iterator over all nodes in allocation order.
    pub fn iter(&self) -> impl Iterator<Item = &Node> {
        self.nodes.iter()
    }
}
