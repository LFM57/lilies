#![deny(unsafe_code)]

use indexmap::IndexMap;

/// Represents tokens produced by the WHATWG HTML Tokenizer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Doctype {
        name: Option<String>,
        public_id: Option<String>,
        system_id: Option<String>,
        force_quirks: bool,
    },
    StartTag {
        name: String,
        attributes: IndexMap<String, String>,
        self_closing: bool,
    },
    EndTag {
        name: String,
    },
    Comment(String),
    Character(char),
    Eof,
}

impl Token {
    pub fn is_eof(&self) -> bool {
        matches!(self, Token::Eof)
    }
}
