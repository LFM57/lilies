#![deny(unsafe_code)]

use crate::arena::{Arena, NodeId};
use crate::error::DomError;
use crate::node::{DocumentType, Element, Node, NodeType, Text};
use indexmap::IndexMap;

/// The root DOM container that holds the arena and coordinates tree mutations.
#[derive(Debug, Clone)]
pub struct Document {
    arena: Arena,
    root: NodeId,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    /// Creates a new DOM document containing a single root Document node.
    pub fn new() -> Self {
        let mut arena = Arena::new();
        let root = arena.alloc(|id| Node::new(id, NodeType::Document));
        Self { arena, root }
    }

    /// Returns the root `NodeId` of the document (always a Document node).
    pub fn root(&self) -> NodeId {
        self.root
    }

    /// Returns an immutable reference to the internal arena.
    pub fn arena(&self) -> &Arena {
        &self.arena
    }

    /// Returns a mutable reference to the internal arena.
    pub fn arena_mut(&mut self) -> &mut Arena {
        &mut self.arena
    }

    /// Retrieves an immutable reference to a node.
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.arena.get(id)
    }

    /// Retrieves a mutable reference to a node.
    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.arena.get_mut(id)
    }

    /// Creates an Element node and returns its `NodeId`.
    pub fn create_element(
        &mut self,
        tag_name: &str,
        attributes: IndexMap<String, String>,
    ) -> NodeId {
        let element = Element::with_attributes(tag_name, attributes);
        self.arena
            .alloc(|id| Node::new(id, NodeType::Element(element)))
    }

    /// Creates a Text node and returns its `NodeId`.
    pub fn create_text(&mut self, data: &str) -> NodeId {
        let text = Text::new(data);
        self.arena.alloc(|id| Node::new(id, NodeType::Text(text)))
    }

    /// Creates a Comment node and returns its `NodeId`.
    pub fn create_comment(&mut self, data: &str) -> NodeId {
        self.arena
            .alloc(|id| Node::new(id, NodeType::Comment(data.to_string())))
    }

    /// Creates a DocumentType node and returns its `NodeId`.
    pub fn create_doctype(
        &mut self,
        name: &str,
        public_id: Option<String>,
        system_id: Option<String>,
    ) -> NodeId {
        let doctype = DocumentType {
            name: name.to_string(),
            public_id,
            system_id,
        };
        self.arena
            .alloc(|id| Node::new(id, NodeType::DocumentType(doctype)))
    }

    /// Creates a DocumentFragment node and returns its `NodeId`.
    pub fn create_document_fragment(&mut self) -> NodeId {
        self.arena
            .alloc(|id| Node::new(id, NodeType::DocumentFragment))
    }

    /// Appends `child` as the last child of `parent`.
    ///
    /// If `child` already has a parent, it is cleanly detached first.
    /// If `child` is a `DocumentFragment`, all of its children are appended in order.
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), DomError> {
        if !self.arena.contains(parent) {
            return Err(DomError::NodeNotFound(parent));
        }
        if !self.arena.contains(child) {
            return Err(DomError::NodeNotFound(child));
        }
        if parent == child || self.is_descendant_of(parent, child) {
            return Err(DomError::HierarchyRequestError);
        }

        if self
            .arena
            .get(child)
            .map_or(false, |n| n.is_document_fragment())
        {
            let frag_children = self.get_children(child);
            for fc in frag_children {
                self.append_child(parent, fc)?;
            }
            return Ok(());
        }

        // If the child already has a parent, detach it
        if self.arena.get(child).and_then(|n| n.parent).is_some() {
            self.detach_from_parent(child)?;
        }

        let old_last = self.arena.get(parent).and_then(|p| p.last_child);

        // Update child node
        if let Some(child_node) = self.arena.get_mut(child) {
            child_node.parent = Some(parent);
            child_node.prev_sibling = old_last;
            child_node.next_sibling = None;
        }

        // Update previous sibling or parent first_child
        if let Some(old_last_id) = old_last {
            if let Some(old_last_node) = self.arena.get_mut(old_last_id) {
                old_last_node.next_sibling = Some(child);
            }
        } else if let Some(parent_node) = self.arena.get_mut(parent) {
            parent_node.first_child = Some(child);
        }

        // Update parent last_child
        if let Some(parent_node) = self.arena.get_mut(parent) {
            parent_node.last_child = Some(child);
        }

        Ok(())
    }

    /// Inserts `child` before `reference_child` as a child of `parent`.
    pub fn insert_before(
        &mut self,
        parent: NodeId,
        child: NodeId,
        reference_child: NodeId,
    ) -> Result<(), DomError> {
        if !self.arena.contains(parent) {
            return Err(DomError::NodeNotFound(parent));
        }
        if !self.arena.contains(child) {
            return Err(DomError::NodeNotFound(child));
        }
        if !self.arena.contains(reference_child) {
            return Err(DomError::NodeNotFound(reference_child));
        }
        if parent == child || self.is_descendant_of(parent, child) {
            return Err(DomError::HierarchyRequestError);
        }
        if self.arena.get(reference_child).and_then(|n| n.parent) != Some(parent) {
            return Err(DomError::InvalidReferenceChild {
                parent,
                reference: reference_child,
            });
        }
        if child == reference_child {
            return Ok(());
        }

        if self
            .arena
            .get(child)
            .map_or(false, |n| n.is_document_fragment())
        {
            let frag_children = self.get_children(child);
            for fc in frag_children {
                self.insert_before(parent, fc, reference_child)?;
            }
            return Ok(());
        }

        // Detach child from existing parent if present
        if self.arena.get(child).and_then(|n| n.parent).is_some() {
            self.detach_from_parent(child)?;
        }

        let ref_prev = self.arena.get(reference_child).and_then(|n| n.prev_sibling);

        // Update child
        if let Some(c) = self.arena.get_mut(child) {
            c.parent = Some(parent);
            c.prev_sibling = ref_prev;
            c.next_sibling = Some(reference_child);
        }

        // Update reference child's prev_sibling
        if let Some(r) = self.arena.get_mut(reference_child) {
            r.prev_sibling = Some(child);
        }

        // Update preceding sibling or parent first_child
        if let Some(prev_id) = ref_prev {
            if let Some(prev_node) = self.arena.get_mut(prev_id) {
                prev_node.next_sibling = Some(child);
            }
        } else if let Some(parent_node) = self.arena.get_mut(parent) {
            parent_node.first_child = Some(child);
        }

        Ok(())
    }

    /// Removes `child` from `parent`.
    pub fn remove_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), DomError> {
        if !self.arena.contains(parent) {
            return Err(DomError::NodeNotFound(parent));
        }
        if !self.arena.contains(child) {
            return Err(DomError::NodeNotFound(child));
        }
        if self.arena.get(child).and_then(|n| n.parent) != Some(parent) {
            return Err(DomError::NotAChild { parent, child });
        }

        self.detach_from_parent(child)
    }

    /// Replaces `old_child` with `new_child` as a child of `parent`.
    pub fn replace_child(
        &mut self,
        parent: NodeId,
        new_child: NodeId,
        old_child: NodeId,
    ) -> Result<(), DomError> {
        if !self.arena.contains(parent) {
            return Err(DomError::NodeNotFound(parent));
        }
        if !self.arena.contains(new_child) {
            return Err(DomError::NodeNotFound(new_child));
        }
        if !self.arena.contains(old_child) {
            return Err(DomError::NodeNotFound(old_child));
        }
        if self.arena.get(old_child).and_then(|n| n.parent) != Some(parent) {
            return Err(DomError::NotAChild {
                parent,
                child: old_child,
            });
        }
        if new_child == old_child {
            return Ok(());
        }

        self.insert_before(parent, new_child, old_child)?;
        self.remove_child(parent, old_child)?;
        Ok(())
    }

    /// Clones a node. If `deep` is true, recursively clones all descendants.
    pub fn clone_node(&mut self, node_id: NodeId, deep: bool) -> Option<NodeId> {
        let node_type = {
            let node = self.arena.get(node_id)?;
            node.node_type.clone()
        };

        let new_id = match node_type {
            NodeType::Document => {
                let doc = Document::new();
                doc.root()
            }
            NodeType::DocumentFragment => self.create_document_fragment(),
            NodeType::DocumentType(dt) => {
                self.create_doctype(&dt.name, dt.public_id, dt.system_id)
            }
            NodeType::Element(el) => self.create_element(&el.tag_name, el.attributes.clone()),
            NodeType::Text(t) => self.create_text(&t.data),
            NodeType::Comment(c) => self.create_comment(&c),
        };

        if deep {
            let children = self.get_children(node_id);
            for child in children {
                if let Some(cloned_child) = self.clone_node(child, true) {
                    let _ = self.append_child(new_id, cloned_child);
                }
            }
        }

        Some(new_id)
    }

    /// Internal helper to detach a node from its parent and siblings.
    pub fn detach_from_parent(&mut self, child: NodeId) -> Result<(), DomError> {
        let (parent_id, prev_sibling, next_sibling) = {
            let child_node = self.arena.get(child).ok_or(DomError::NodeNotFound(child))?;
            (
                child_node.parent,
                child_node.prev_sibling,
                child_node.next_sibling,
            )
        };

        let Some(parent_id) = parent_id else {
            return Ok(());
        };

        // Link prev_sibling to next_sibling
        if let Some(prev_id) = prev_sibling {
            if let Some(prev_node) = self.arena.get_mut(prev_id) {
                prev_node.next_sibling = next_sibling;
            }
        } else if let Some(parent_node) = self.arena.get_mut(parent_id) {
            parent_node.first_child = next_sibling;
        }

        // Link next_sibling to prev_sibling
        if let Some(next_id) = next_sibling {
            if let Some(next_node) = self.arena.get_mut(next_id) {
                next_node.prev_sibling = prev_sibling;
            }
        } else if let Some(parent_node) = self.arena.get_mut(parent_id) {
            parent_node.last_child = prev_sibling;
        }

        // Clear child relations
        if let Some(child_node) = self.arena.get_mut(child) {
            child_node.parent = None;
            child_node.prev_sibling = None;
            child_node.next_sibling = None;
        }

        Ok(())
    }

    /// Checks if `descendant` is a descendant of `ancestor`.
    pub fn is_descendant_of(&self, descendant: NodeId, ancestor: NodeId) -> bool {
        let mut curr = self.arena.get(descendant).and_then(|n| n.parent);
        while let Some(parent_id) = curr {
            if parent_id == ancestor {
                return true;
            }
            curr = self.arena.get(parent_id).and_then(|n| n.parent);
        }
        false
    }

    /// Returns a list of direct children for the given node.
    pub fn get_children(&self, parent: NodeId) -> Vec<NodeId> {
        let mut children = Vec::new();
        let mut current = self.arena.get(parent).and_then(|n| n.first_child);
        while let Some(child_id) = current {
            children.push(child_id);
            current = self.arena.get(child_id).and_then(|n| n.next_sibling);
        }
        children
    }

    /// Returns the parent of a node, if any.
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.arena.get(id).and_then(|n| n.parent)
    }

    /// Returns the first child of a node, if any.
    pub fn first_child(&self, id: NodeId) -> Option<NodeId> {
        self.arena.get(id).and_then(|n| n.first_child)
    }

    /// Returns the last child of a node, if any.
    pub fn last_child(&self, id: NodeId) -> Option<NodeId> {
        self.arena.get(id).and_then(|n| n.last_child)
    }

    /// Returns the next sibling of a node, if any.
    pub fn next_sibling(&self, id: NodeId) -> Option<NodeId> {
        self.arena.get(id).and_then(|n| n.next_sibling)
    }

    /// Returns the previous sibling of a node, if any.
    pub fn prev_sibling(&self, id: NodeId) -> Option<NodeId> {
        self.arena.get(id).and_then(|n| n.prev_sibling)
    }

    /// Concatenates all descendant text nodes under `id`.
    pub fn text_content(&self, id: NodeId) -> String {
        let mut result = String::new();
        self.collect_text(id, &mut result);
        result
    }

    fn collect_text(&self, id: NodeId, out: &mut String) {
        if let Some(node) = self.arena.get(id) {
            if let NodeType::Text(text) = &node.node_type {
                out.push_str(&text.data);
            }
            for child in self.get_children(id) {
                self.collect_text(child, out);
            }
        }
    }

    /// Returns an indented string representation of the tree starting at `root_id`.
    pub fn dump_tree(&self, root_id: NodeId) -> String {
        let mut out = String::new();
        self.dump_node_recursive(root_id, 0, &mut out);
        if out.ends_with('\n') {
            out.pop();
        }
        out
    }

    /// Returns an indented string representation of the entire document tree.
    pub fn dump_document(&self) -> String {
        self.dump_tree(self.root)
    }

    fn dump_node_recursive(&self, node_id: NodeId, depth: usize, out: &mut String) {
        let Some(node) = self.arena.get(node_id) else {
            return;
        };

        let indent = "  ".repeat(depth);
        match &node.node_type {
            NodeType::Document => {
                out.push_str(&format!("{indent}#document\n"));
            }
            NodeType::DocumentType(doctype) => {
                out.push_str(&format!("{indent}<!DOCTYPE {}>\n", doctype.name));
            }
            NodeType::Element(el) => {
                if el.attributes.is_empty() {
                    out.push_str(&format!("{indent}<{}>\n", el.tag_name));
                } else {
                    let attrs_str: Vec<String> = el
                        .attributes
                        .iter()
                        .map(|(k, v)| format!("{k}=\"{v}\""))
                        .collect();
                    out.push_str(&format!(
                        "{indent}<{} {}>\n",
                        el.tag_name,
                        attrs_str.join(" ")
                    ));
                }
            }
            NodeType::Text(text) => {
                out.push_str(&format!("{indent}#text \"{}\"\n", text.data));
            }
            NodeType::Comment(comment) => {
                out.push_str(&format!("{indent}<!--{comment}-->\n"));
            }
            NodeType::DocumentFragment => {
                out.push_str(&format!("{indent}#document-fragment\n"));
            }
        }

        for child_id in self.get_children(node_id) {
            self.dump_node_recursive(child_id, depth + 1, out);
        }
    }
}
