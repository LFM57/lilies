#![deny(unsafe_code)]

use crate::arena::NodeId;
use indexmap::IndexMap;

/// Represents an HTML Element node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub tag_name: String,
    pub attributes: IndexMap<String, String>,
}

impl Element {
    /// Creates a new element with no attributes.
    pub fn new(tag_name: impl Into<String>) -> Self {
        Self {
            tag_name: tag_name.into(),
            attributes: IndexMap::new(),
        }
    }

    /// Creates a new element with initial attributes.
    pub fn with_attributes(
        tag_name: impl Into<String>,
        attributes: IndexMap<String, String>,
    ) -> Self {
        Self {
            tag_name: tag_name.into(),
            attributes,
        }
    }

    /// Returns the element's tag name.
    pub fn tag_name(&self) -> &str {
        &self.tag_name
    }

    /// Returns the attribute value if present.
    pub fn get_attribute(&self, name: &str) -> Option<&str> {
        self.attributes.get(name).map(|s| s.as_str())
    }

    /// Sets or updates an attribute.
    pub fn set_attribute(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.attributes.insert(name.into(), value.into());
    }

    /// Removes an attribute if present.
    pub fn remove_attribute(&mut self, name: &str) -> Option<String> {
        self.attributes.swap_remove(name)
    }

    /// Checks if an attribute is present.
    pub fn has_attribute(&self, name: &str) -> bool {
        self.attributes.contains_key(name)
    }

    /// Returns the value of the `id` attribute, if any.
    pub fn id(&self) -> Option<&str> {
        self.get_attribute("id")
    }

    /// Returns an iterator over the class names split by ASCII whitespace.
    pub fn classes(&self) -> Vec<&str> {
        self.get_attribute("class")
            .map(|c| c.split_ascii_whitespace().collect())
            .unwrap_or_default()
    }

    /// Checks if the element has the specified class.
    pub fn has_class(&self, class: &str) -> bool {
        self.classes().contains(&class)
    }
}

/// Represents a Text node in the DOM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text {
    pub data: String,
}

impl Text {
    pub fn new(data: impl Into<String>) -> Self {
        Self { data: data.into() }
    }

    pub fn data(&self) -> &str {
        &self.data
    }

    pub fn append(&mut self, text: &str) {
        self.data.push_str(text);
    }
}

/// Represents a DocumentType node (e.g., `<!DOCTYPE html>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentType {
    pub name: String,
    pub public_id: Option<String>,
    pub system_id: Option<String>,
}

/// The specific data associated with a DOM node.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeType {
    Document,
    DocumentType(DocumentType),
    Element(Element),
    Text(Text),
    Comment(String),
    DocumentFragment,
}

/// A node within the DOM tree.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub id: NodeId,
    pub node_type: NodeType,
    pub parent: Option<NodeId>,
    pub first_child: Option<NodeId>,
    pub last_child: Option<NodeId>,
    pub next_sibling: Option<NodeId>,
    pub prev_sibling: Option<NodeId>,
}

impl Node {
    pub fn new(id: NodeId, node_type: NodeType) -> Self {
        Self {
            id,
            node_type,
            parent: None,
            first_child: None,
            last_child: None,
            next_sibling: None,
            prev_sibling: None,
        }
    }

    pub fn is_document(&self) -> bool {
        matches!(self.node_type, NodeType::Document)
    }

    pub fn is_element(&self) -> bool {
        matches!(self.node_type, NodeType::Element(_))
    }

    pub fn is_document_fragment(&self) -> bool {
        matches!(self.node_type, NodeType::DocumentFragment)
    }

    pub fn as_element(&self) -> Option<&Element> {
        match &self.node_type {
            NodeType::Element(el) => Some(el),
            _ => None,
        }
    }

    pub fn as_element_mut(&mut self) -> Option<&mut Element> {
        match &mut self.node_type {
            NodeType::Element(el) => Some(el),
            _ => None,
        }
    }

    pub fn is_text(&self) -> bool {
        matches!(self.node_type, NodeType::Text(_))
    }

    pub fn as_text(&self) -> Option<&Text> {
        match &self.node_type {
            NodeType::Text(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_text_mut(&mut self) -> Option<&mut Text> {
        match &mut self.node_type {
            NodeType::Text(text) => Some(text),
            _ => None,
        }
    }

    pub fn is_comment(&self) -> bool {
        matches!(self.node_type, NodeType::Comment(_))
    }

    pub fn as_comment(&self) -> Option<&str> {
        match &self.node_type {
            NodeType::Comment(comment) => Some(comment.as_str()),
            _ => None,
        }
    }
}
