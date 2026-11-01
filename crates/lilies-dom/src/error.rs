#![deny(unsafe_code)]

use crate::arena::NodeId;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum DomError {
    #[error("Node {0} was not found in arena")]
    NodeNotFound(NodeId),

    #[error("Hierarchy error: cannot make a node a child of itself or its descendant")]
    HierarchyRequestError,

    #[error("Child {child} is not a child of parent {parent}")]
    NotAChild { parent: NodeId, child: NodeId },

    #[error("Reference child {reference} is not a child of parent {parent}")]
    InvalidReferenceChild { parent: NodeId, reference: NodeId },
}
