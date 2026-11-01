#![deny(unsafe_code)]

use crate::ast::Selector;
use std::fmt;

/// CSS Specificity tuple: (ID count, Class count, Tag/Type count).
///
/// Implements lexicographical comparison where IDs > Classes > Tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Specificity(pub usize, pub usize, pub usize);

impl Specificity {
    pub const fn new(id_count: usize, class_count: usize, tag_count: usize) -> Self {
        Self(id_count, class_count, tag_count)
    }
}

impl From<Specificity> for (usize, usize, usize) {
    fn from(s: Specificity) -> Self {
        (s.0, s.1, s.2)
    }
}

impl fmt::Display for Specificity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {}, {})", self.0, self.1, self.2)
    }
}

impl Selector {
    /// Computes the specificity of this selector: (ID count, Class count, Tag count).
    pub fn specificity(&self) -> Specificity {
        let mut id_count = if self.id.is_some() { 1 } else { 0 };
        let mut class_count = self.classes.len()
            + self
                .pseudo_classes
                .iter()
                .filter(|p| !p.starts_with("::"))
                .count();
        let mut tag_count = if self.tag_name.is_some() { 1 } else { 0 }
            + self
                .pseudo_classes
                .iter()
                .filter(|p| p.starts_with("::"))
                .count();

        if let Some((_, ref parent)) = self.parent {
            let p_spec = parent.specificity();
            id_count += p_spec.0;
            class_count += p_spec.1;
            tag_count += p_spec.2;
        }

        Specificity::new(id_count, class_count, tag_count)
    }
}
