#![deny(unsafe_code)]

use crate::parser::is_void_element;
use crate::token::Token;
use crate::tokenizer::Tokenizer;
use lilies_dom::{Document, NodeId};

/// Parses an HTML input string into a `Document` tree with WHATWG-compliant auto-closing.
pub fn parse_html(input: &str) -> Document {
    let mut doc = Document::new();
    let root = doc.root();
    let mut stack: Vec<NodeId> = vec![root];

    let tokenizer = Tokenizer::new(input);

    for token in tokenizer {
        match token {
            Token::Doctype {
                name,
                public_id,
                system_id,
                ..
            } => {
                let doctype_id =
                    doc.create_doctype(&name.unwrap_or_default(), public_id, system_id);
                let _ = doc.append_child(root, doctype_id);
            }

            Token::StartTag {
                name,
                attributes,
                self_closing,
            } => {
                // In WHATWG HTML, certain elements auto-close if another open tag of the same kind is encountered.
                // Specifically: When encountering an open <p> tag, if there is already an active <p> element
                // on the open-elements stack, pop/close it before opening the new <p>.
                if name.eq_ignore_ascii_case("p") {
                    if let Some(p_idx) = stack.iter().rposition(|&id| {
                        if id == root {
                            return false;
                        }
                        doc.node(id)
                            .and_then(|n| n.as_element())
                            .is_some_and(|el| el.tag_name().eq_ignore_ascii_case("p"))
                    }) {
                        stack.truncate(p_idx);
                    }
                } else if name.eq_ignore_ascii_case("li") {
                    // Auto-close preceding <li> inside the same list (stop at enclosing <ul>, <ol>, or <menu>)
                    let mut li_idx = None;
                    for (idx, &id) in stack.iter().enumerate().rev() {
                        if id == root {
                            break;
                        }
                        if let Some(el) = doc.node(id).and_then(|n| n.as_element()) {
                            let tag = el.tag_name();
                            if tag.eq_ignore_ascii_case("li") {
                                li_idx = Some(idx);
                                break;
                            }
                            if tag.eq_ignore_ascii_case("ul")
                                || tag.eq_ignore_ascii_case("ol")
                                || tag.eq_ignore_ascii_case("menu")
                            {
                                break;
                            }
                        }
                    }
                    if let Some(idx) = li_idx {
                        stack.truncate(idx);
                    }
                }

                let elem_id = doc.create_element(&name, attributes);
                let current_parent = *stack.last().unwrap_or(&root);
                let _ = doc.append_child(current_parent, elem_id);

                let is_void = is_void_element(&name);
                if !self_closing && !is_void {
                    stack.push(elem_id);
                }
            }

            Token::EndTag { name } => {
                // Find matching tag in stack from innermost to outermost (excluding document root)
                let match_idx = stack.iter().rposition(|&id| {
                    if id == root {
                        return false;
                    }
                    if let Some(node) = doc.node(id) {
                        if let Some(el) = node.as_element() {
                            return el.tag_name().eq_ignore_ascii_case(&name);
                        }
                    }
                    false
                });

                if let Some(idx) = match_idx {
                    stack.truncate(idx);
                }
            }

            Token::Character(c) => {
                let current_parent = *stack.last().unwrap_or(&root);
                let last_child = doc.last_child(current_parent);

                // Merge adjacent character tokens into a single text node
                let merged = if let Some(last_child_id) = last_child {
                    if let Some(node) = doc.node_mut(last_child_id) {
                        if let Some(text) = node.as_text_mut() {
                            text.append(&c.to_string());
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                };

                if !merged {
                    let text_id = doc.create_text(&c.to_string());
                    let _ = doc.append_child(current_parent, text_id);
                }
            }

            Token::Comment(data) => {
                let comment_id = doc.create_comment(&data);
                let current_parent = *stack.last().unwrap_or(&root);
                let _ = doc.append_child(current_parent, comment_id);
            }

            Token::Eof => {
                break;
            }
        }
    }

    doc
}
