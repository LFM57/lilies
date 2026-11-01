#![deny(unsafe_code)]

use crate::token::Token;
use indexmap::IndexMap;
use std::collections::VecDeque;

/// WHATWG HTML Tokenizer states (Milestone 1 subset).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenizerState {
    Data,
    TagOpen,
    EndTagOpen,
    TagName,
    BeforeAttributeName,
    AttributeName,
    AfterAttributeName,
    BeforeAttributeValue,
    AttributeValueDoubleQuoted,
    AttributeValueSingleQuoted,
    AttributeValueUnquoted,
    AfterAttributeValueQuoted,
    SelfClosingStartTag,
    MarkupDeclarationOpen,
    CommentStart,
    CommentStartDash,
    Comment,
    CommentEndDash,
    CommentEnd,
    CommentEndBang,
    BeforeDoctypeName,
    DoctypeName,
    AfterDoctypeName,
    BogusComment,
    RawText,
}

struct InputStream {
    chars: Vec<char>,
    pos: usize,
}

impl InputStream {
    fn new(input: &str) -> Self {
        Self {
            chars: input.chars().collect(),
            pos: 0,
        }
    }

    fn next(&mut self) -> Option<char> {
        if self.pos < self.chars.len() {
            let ch = self.chars[self.pos];
            self.pos += 1;
            Some(ch)
        } else {
            None
        }
    }

    fn reconsume(&mut self) {
        if self.pos > 0 {
            self.pos -= 1;
        }
    }

    fn match_case_insensitive_str(&mut self, s: &str) -> bool {
        let count = s.chars().count();
        if self.pos + count <= self.chars.len() {
            let slice = &self.chars[self.pos..self.pos + count];
            for (c1, c2) in slice.iter().zip(s.chars()) {
                if !c1.eq_ignore_ascii_case(&c2) {
                    return false;
                }
            }
            self.pos += count;
            true
        } else {
            false
        }
    }

    fn match_exact_str(&mut self, s: &str) -> bool {
        let count = s.chars().count();
        if self.pos + count <= self.chars.len() {
            let slice = &self.chars[self.pos..self.pos + count];
            for (c1, c2) in slice.iter().zip(s.chars()) {
                if *c1 != c2 {
                    return false;
                }
            }
            self.pos += count;
            true
        } else {
            false
        }
    }

    /// Attempts to consume an HTML character reference immediately after `'&'` has been consumed.
    /// If valid, advances `self.pos` past the reference (including trailing `;`) and returns the decoded string.
    /// Otherwise leaves `self.pos` unchanged and returns `None`.
    fn try_consume_char_ref(&mut self) -> Option<String> {
        let start_pos = self.pos;

        if self.chars.get(start_pos) == Some(&'#') {
            let mut p = start_pos + 1;
            let (radix, digit_start) = if matches!(self.chars.get(p), Some('x' | 'X')) {
                p += 1;
                while self.chars.get(p).is_some_and(|c| c.is_ascii_hexdigit()) {
                    p += 1;
                }
                (16, start_pos + 2)
            } else {
                while self.chars.get(p).is_some_and(|c| c.is_ascii_digit()) {
                    p += 1;
                }
                (10, start_pos + 1)
            };

            if p > digit_start {
                let num_str: String = self.chars[digit_start..p].iter().collect();
                if self.chars.get(p) == Some(&';') {
                    p += 1;
                }
                if let Ok(code) = u32::from_str_radix(&num_str, radix) {
                    let ch = if code == 0 {
                        '\u{FFFD}'
                    } else {
                        char::from_u32(code).unwrap_or('\u{FFFD}')
                    };
                    self.pos = p;
                    return Some(ch.to_string());
                }
            }
            return None;
        }

        let mut p = start_pos;
        while self.chars.get(p).is_some_and(|c| c.is_ascii_alphanumeric()) {
            p += 1;
        }

        if p > start_pos && self.chars.get(p) == Some(&';') {
            let name: String = self.chars[start_pos..p].iter().collect();
            if let Some(decoded) = lookup_named_entity(&name) {
                self.pos = p + 1;
                return Some(decoded.to_string());
            }
        }

        None
    }
}

fn lookup_named_entity(name: &str) -> Option<&'static str> {
    match name {
        "amp" | "AMP" => Some("&"),
        "lt" | "LT" => Some("<"),
        "gt" | "GT" => Some(">"),
        "quot" | "QUOT" => Some("\""),
        "apos" => Some("'"),
        "nbsp" | "NBSP" => Some("\u{00A0}"),
        "ensp" => Some("\u{2002}"),
        "emsp" => Some("\u{2003}"),
        "thinsp" => Some("\u{2009}"),
        "zwnj" => Some("\u{200C}"),
        "zwj" => Some("\u{200D}"),
        "ndash" => Some("\u{2013}"),
        "mdash" => Some("\u{2014}"),
        "hellip" => Some("\u{2026}"),
        "lsquo" => Some("\u{2018}"),
        "rsquo" => Some("\u{2019}"),
        "ldquo" => Some("\u{201C}"),
        "rdquo" => Some("\u{201D}"),
        "laquo" => Some("\u{00AB}"),
        "raquo" => Some("\u{00BB}"),
        "bull" => Some("\u{2022}"),
        "middot" => Some("\u{00B7}"),
        "sect" => Some("\u{00A7}"),
        "para" => Some("\u{00B6}"),
        "dagger" => Some("\u{2020}"),
        "Dagger" => Some("\u{2021}"),
        "copy" | "COPY" => Some("\u{00A9}"),
        "reg" | "REG" => Some("\u{00AE}"),
        "trade" | "TRADE" => Some("\u{2122}"),
        "deg" => Some("\u{00B0}"),
        "plusmn" => Some("\u{00B1}"),
        "times" => Some("\u{00D7}"),
        "divide" => Some("\u{00F7}"),
        "frac12" => Some("\u{00BD}"),
        "frac14" => Some("\u{00BC}"),
        "frac34" => Some("\u{00BE}"),
        "cent" => Some("\u{00A2}"),
        "pound" => Some("\u{00A3}"),
        "yen" => Some("\u{00A5}"),
        "euro" => Some("\u{20AC}"),
        "larr" => Some("\u{2190}"),
        "uarr" => Some("\u{2191}"),
        "rarr" => Some("\u{2192}"),
        "darr" => Some("\u{2193}"),
        "harr" => Some("\u{2194}"),
        "hearts" => Some("\u{2665}"),
        "spades" => Some("\u{2660}"),
        "clubs" => Some("\u{2663}"),
        "diams" => Some("\u{2666}"),
        "check" => Some("\u{2713}"),
        _ => None,
    }
}

/// Tokenizer that parses an HTML string into a stream of `Token`s
/// conforming to the WHATWG HTML Tokenization state machine.
pub struct Tokenizer {
    stream: InputStream,
    pub state: TokenizerState,
    raw_tag_name: String,

    // Current tag being constructed
    current_tag_name: String,
    current_attributes: IndexMap<String, String>,
    current_attr_name: String,
    current_attr_value: String,
    current_self_closing: bool,
    is_end_tag: bool,

    // Current comment being constructed
    current_comment: String,

    // Current doctype being constructed
    current_doctype_name: String,
    current_doctype_force_quirks: bool,

    // Token queue
    tokens: VecDeque<Token>,
    eof_emitted: bool,
}

impl Tokenizer {
    pub fn new(input: &str) -> Self {
        Self {
            stream: InputStream::new(input),
            state: TokenizerState::Data,
            raw_tag_name: String::new(),
            current_tag_name: String::new(),
            current_attributes: IndexMap::new(),
            current_attr_name: String::new(),
            current_attr_value: String::new(),
            current_self_closing: false,
            is_end_tag: false,
            current_comment: String::new(),
            current_doctype_name: String::new(),
            current_doctype_force_quirks: false,
            tokens: VecDeque::new(),
            eof_emitted: false,
        }
    }

    fn commit_attribute(&mut self) {
        if !self.current_attr_name.is_empty() {
            let name = std::mem::take(&mut self.current_attr_name);
            let val = std::mem::take(&mut self.current_attr_value);
            // According to WHATWG, duplicate attributes are ignored (first one wins)
            self.current_attributes.entry(name).or_insert(val);
        }
    }

    fn emit_current_tag(&mut self) {
        self.commit_attribute();
        let name = std::mem::take(&mut self.current_tag_name);
        let attributes = std::mem::take(&mut self.current_attributes);
        let self_closing = self.current_self_closing;
        self.current_self_closing = false;

        if self.is_end_tag {
            self.tokens.push_back(Token::EndTag { name });
        } else {
            if !self_closing {
                let lower = name.to_ascii_lowercase();
                if matches!(lower.as_str(), "script" | "style" | "title" | "textarea") {
                    self.state = TokenizerState::RawText;
                    self.raw_tag_name = lower;
                }
            }
            self.tokens.push_back(Token::StartTag {
                name,
                attributes,
                self_closing,
            });
        }
    }

    /// Advances the state machine by processing the next character.
    fn step(&mut self) {
        let ch = self.stream.next();

        match self.state {
            TokenizerState::Data => match ch {
                Some('<') => {
                    self.state = TokenizerState::TagOpen;
                }
                Some('&') => {
                    if let Some(decoded) = self.stream.try_consume_char_ref() {
                        for dc in decoded.chars() {
                            self.tokens.push_back(Token::Character(dc));
                        }
                    } else {
                        self.tokens.push_back(Token::Character('&'));
                    }
                }
                Some(c) => {
                    self.tokens.push_back(Token::Character(c));
                }
                None => {
                    self.tokens.push_back(Token::Eof);
                    self.eof_emitted = true;
                }
            },

            TokenizerState::TagOpen => match ch {
                Some('!') => {
                    self.state = TokenizerState::MarkupDeclarationOpen;
                }
                Some('/') => {
                    self.state = TokenizerState::EndTagOpen;
                }
                Some(c) if c.is_ascii_alphabetic() => {
                    self.is_end_tag = false;
                    self.current_tag_name = c.to_ascii_lowercase().to_string();
                    self.current_attributes.clear();
                    self.current_self_closing = false;
                    self.state = TokenizerState::TagName;
                }
                Some('?') => {
                    self.current_comment.clear();
                    self.state = TokenizerState::BogusComment;
                }
                Some(_) => {
                    self.tokens.push_back(Token::Character('<'));
                    self.stream.reconsume();
                    self.state = TokenizerState::Data;
                }
                None => {
                    self.tokens.push_back(Token::Character('<'));
                    self.tokens.push_back(Token::Eof);
                    self.eof_emitted = true;
                }
            },

            TokenizerState::EndTagOpen => match ch {
                Some(c) if c.is_ascii_alphabetic() => {
                    self.is_end_tag = true;
                    self.current_tag_name = c.to_ascii_lowercase().to_string();
                    self.current_attributes.clear();
                    self.current_self_closing = false;
                    self.state = TokenizerState::TagName;
                }
                Some('>') => {
                    self.state = TokenizerState::Data;
                }
                Some(_) => {
                    self.current_comment.clear();
                    self.stream.reconsume();
                    self.state = TokenizerState::BogusComment;
                }
                None => {
                    self.tokens.push_back(Token::Character('<'));
                    self.tokens.push_back(Token::Character('/'));
                    self.tokens.push_back(Token::Eof);
                    self.eof_emitted = true;
                }
            },

            TokenizerState::TagName => match ch {
                Some(c) if c.is_ascii_whitespace() => {
                    self.state = TokenizerState::BeforeAttributeName;
                }
                Some('/') => {
                    self.state = TokenizerState::SelfClosingStartTag;
                }
                Some('>') => {
                    self.state = TokenizerState::Data;
                    self.emit_current_tag();
                }
                Some(c) => {
                    self.current_tag_name.push(c.to_ascii_lowercase());
                }
                None => {
                    self.tokens.push_back(Token::Eof);
                    self.eof_emitted = true;
                }
            },

            TokenizerState::BeforeAttributeName => match ch {
                Some(c) if c.is_ascii_whitespace() => {
                    // Ignore whitespace
                }
                Some('/') | Some('>') | None => {
                    self.stream.reconsume();
                    self.state = TokenizerState::AfterAttributeName;
                }
                Some('=') => {
                    self.current_attr_name = "=".to_string();
                    self.current_attr_value.clear();
                    self.state = TokenizerState::AttributeName;
                }
                Some(c) => {
                    self.current_attr_name = c.to_ascii_lowercase().to_string();
                    self.current_attr_value.clear();
                    self.state = TokenizerState::AttributeName;
                }
            },

            TokenizerState::AttributeName => match ch {
                Some(c) if c.is_ascii_whitespace() || c == '/' || c == '>' => {
                    self.stream.reconsume();
                    self.state = TokenizerState::AfterAttributeName;
                }
                Some('=') => {
                    self.state = TokenizerState::BeforeAttributeValue;
                }
                Some(c) => {
                    self.current_attr_name.push(c.to_ascii_lowercase());
                }
                None => {
                    self.stream.reconsume();
                    self.state = TokenizerState::AfterAttributeName;
                }
            },

            TokenizerState::AfterAttributeName => match ch {
                Some(c) if c.is_ascii_whitespace() => {
                    // Ignore whitespace
                }
                Some('/') => {
                    self.commit_attribute();
                    self.state = TokenizerState::SelfClosingStartTag;
                }
                Some('=') => {
                    self.state = TokenizerState::BeforeAttributeValue;
                }
                Some('>') => {
                    self.state = TokenizerState::Data;
                    self.emit_current_tag();
                }
                Some(c) => {
                    self.commit_attribute();
                    self.current_attr_name = c.to_ascii_lowercase().to_string();
                    self.current_attr_value.clear();
                    self.state = TokenizerState::AttributeName;
                }
                None => {
                    self.commit_attribute();
                    self.tokens.push_back(Token::Eof);
                    self.eof_emitted = true;
                }
            },

            TokenizerState::BeforeAttributeValue => match ch {
                Some(c) if c.is_ascii_whitespace() => {
                    // Ignore whitespace
                }
                Some('"') => {
                    self.state = TokenizerState::AttributeValueDoubleQuoted;
                }
                Some('\'') => {
                    self.state = TokenizerState::AttributeValueSingleQuoted;
                }
                Some('>') => {
                    self.state = TokenizerState::Data;
                    self.emit_current_tag();
                }
                Some(_) => {
                    self.stream.reconsume();
                    self.state = TokenizerState::AttributeValueUnquoted;
                }
                None => {
                    self.state = TokenizerState::Data;
                    self.emit_current_tag();
                    self.tokens.push_back(Token::Eof);
                    self.eof_emitted = true;
                }
            },

            TokenizerState::AttributeValueDoubleQuoted => match ch {
                Some('"') => {
                    self.commit_attribute();
                    self.state = TokenizerState::AfterAttributeValueQuoted;
                }
                Some('&') => {
                    if let Some(decoded) = self.stream.try_consume_char_ref() {
                        self.current_attr_value.push_str(&decoded);
                    } else {
                        self.current_attr_value.push('&');
                    }
                }
                Some(c) => {
                    self.current_attr_value.push(c);
                }
                None => {
                    self.commit_attribute();
                    self.tokens.push_back(Token::Eof);
                    self.eof_emitted = true;
                }
            },

            TokenizerState::AttributeValueSingleQuoted => match ch {
                Some('\'') => {
                    self.commit_attribute();
                    self.state = TokenizerState::AfterAttributeValueQuoted;
                }
                Some('&') => {
                    if let Some(decoded) = self.stream.try_consume_char_ref() {
                        self.current_attr_value.push_str(&decoded);
                    } else {
                        self.current_attr_value.push('&');
                    }
                }
                Some(c) => {
                    self.current_attr_value.push(c);
                }
                None => {
                    self.commit_attribute();
                    self.tokens.push_back(Token::Eof);
                    self.eof_emitted = true;
                }
            },

            TokenizerState::AttributeValueUnquoted => match ch {
                Some(c) if c.is_ascii_whitespace() => {
                    self.commit_attribute();
                    self.state = TokenizerState::BeforeAttributeName;
                }
                Some('>') => {
                    self.state = TokenizerState::Data;
                    self.emit_current_tag();
                }
                Some('&') => {
                    if let Some(decoded) = self.stream.try_consume_char_ref() {
                        self.current_attr_value.push_str(&decoded);
                    } else {
                        self.current_attr_value.push('&');
                    }
                }
                Some(c) => {
                    self.current_attr_value.push(c);
                }
                None => {
                    self.commit_attribute();
                    self.tokens.push_back(Token::Eof);
                    self.eof_emitted = true;
                }
            },

            TokenizerState::AfterAttributeValueQuoted => match ch {
                Some(c) if c.is_ascii_whitespace() => {
                    self.state = TokenizerState::BeforeAttributeName;
                }
                Some('/') => {
                    self.state = TokenizerState::SelfClosingStartTag;
                }
                Some('>') => {
                    self.state = TokenizerState::Data;
                    self.emit_current_tag();
                }
                Some(_) => {
                    self.stream.reconsume();
                    self.state = TokenizerState::BeforeAttributeName;
                }
                None => {
                    self.tokens.push_back(Token::Eof);
                    self.eof_emitted = true;
                }
            },

            TokenizerState::SelfClosingStartTag => match ch {
                Some('>') => {
                    self.current_self_closing = true;
                    self.state = TokenizerState::Data;
                    self.emit_current_tag();
                }
                Some(_) => {
                    self.stream.reconsume();
                    self.state = TokenizerState::BeforeAttributeName;
                }
                None => {
                    self.tokens.push_back(Token::Eof);
                    self.eof_emitted = true;
                }
            },

            TokenizerState::MarkupDeclarationOpen => {
                self.stream.reconsume();
                if self.stream.match_exact_str("--") {
                    self.current_comment.clear();
                    self.state = TokenizerState::CommentStart;
                } else if self.stream.match_case_insensitive_str("DOCTYPE") {
                    self.current_doctype_name.clear();
                    self.current_doctype_force_quirks = false;
                    self.state = TokenizerState::BeforeDoctypeName;
                } else {
                    self.current_comment.clear();
                    self.state = TokenizerState::BogusComment;
                }
            }

            TokenizerState::CommentStart => match ch {
                Some('-') => {
                    self.state = TokenizerState::CommentStartDash;
                }
                Some('>') => {
                    self.state = TokenizerState::Data;
                    self.tokens.push_back(Token::Comment(String::new()));
                }
                Some(_) => {
                    self.stream.reconsume();
                    self.state = TokenizerState::Comment;
                }
                None => {
                    self.tokens.push_back(Token::Comment(String::new()));
                    self.tokens.push_back(Token::Eof);
                    self.eof_emitted = true;
                }
            },

            TokenizerState::CommentStartDash => match ch {
                Some('-') => {
                    self.state = TokenizerState::CommentEnd;
                }
                Some('>') => {
                    self.state = TokenizerState::Data;
                    self.tokens.push_back(Token::Comment(String::new()));
                }
                Some(_) => {
                    self.current_comment.push('-');
                    self.stream.reconsume();
                    self.state = TokenizerState::Comment;
                }
                None => {
                    self.tokens.push_back(Token::Comment(String::new()));
                    self.tokens.push_back(Token::Eof);
                    self.eof_emitted = true;
                }
            },

            TokenizerState::Comment => match ch {
                Some('-') => {
                    self.state = TokenizerState::CommentEndDash;
                }
                Some(c) => {
                    self.current_comment.push(c);
                }
                None => {
                    let comment = std::mem::take(&mut self.current_comment);
                    self.tokens.push_back(Token::Comment(comment));
                    self.tokens.push_back(Token::Eof);
                    self.eof_emitted = true;
                }
            },

            TokenizerState::CommentEndDash => match ch {
                Some('-') => {
                    self.state = TokenizerState::CommentEnd;
                }
                Some(_) => {
                    self.current_comment.push('-');
                    self.stream.reconsume();
                    self.state = TokenizerState::Comment;
                }
                None => {
                    let comment = std::mem::take(&mut self.current_comment);
                    self.tokens.push_back(Token::Comment(comment));
                    self.tokens.push_back(Token::Eof);
                    self.eof_emitted = true;
                }
            },

            TokenizerState::CommentEnd => match ch {
                Some('>') => {
                    self.state = TokenizerState::Data;
                    let comment = std::mem::take(&mut self.current_comment);
                    self.tokens.push_back(Token::Comment(comment));
                }
                Some('!') => {
                    self.state = TokenizerState::CommentEndBang;
                }
                Some('-') => {
                    self.current_comment.push('-');
                }
                Some(_) => {
                    self.current_comment.push_str("--");
                    self.stream.reconsume();
                    self.state = TokenizerState::Comment;
                }
                None => {
                    let comment = std::mem::take(&mut self.current_comment);
                    self.tokens.push_back(Token::Comment(comment));
                    self.tokens.push_back(Token::Eof);
                    self.eof_emitted = true;
                }
            },

            TokenizerState::CommentEndBang => match ch {
                Some('>') => {
                    self.state = TokenizerState::Data;
                    let comment = std::mem::take(&mut self.current_comment);
                    self.tokens.push_back(Token::Comment(comment));
                }
                Some('-') => {
                    self.current_comment.push_str("--!");
                    self.state = TokenizerState::CommentEndDash;
                }
                Some(_) => {
                    self.current_comment.push_str("--!");
                    self.stream.reconsume();
                    self.state = TokenizerState::Comment;
                }
                None => {
                    let comment = std::mem::take(&mut self.current_comment);
                    self.tokens.push_back(Token::Comment(comment));
                    self.tokens.push_back(Token::Eof);
                    self.eof_emitted = true;
                }
            },

            TokenizerState::BeforeDoctypeName => match ch {
                Some(c) if c.is_ascii_whitespace() => {
                    // Ignore whitespace
                }
                Some('>') => {
                    self.state = TokenizerState::Data;
                    self.tokens.push_back(Token::Doctype {
                        name: None,
                        public_id: None,
                        system_id: None,
                        force_quirks: true,
                    });
                }
                Some(c) => {
                    self.current_doctype_name = c.to_ascii_lowercase().to_string();
                    self.state = TokenizerState::DoctypeName;
                }
                None => {
                    self.tokens.push_back(Token::Doctype {
                        name: None,
                        public_id: None,
                        system_id: None,
                        force_quirks: true,
                    });
                    self.tokens.push_back(Token::Eof);
                    self.eof_emitted = true;
                }
            },

            TokenizerState::DoctypeName => match ch {
                Some(c) if c.is_ascii_whitespace() => {
                    self.state = TokenizerState::AfterDoctypeName;
                }
                Some('>') => {
                    self.state = TokenizerState::Data;
                    let name = std::mem::take(&mut self.current_doctype_name);
                    self.tokens.push_back(Token::Doctype {
                        name: Some(name),
                        public_id: None,
                        system_id: None,
                        force_quirks: self.current_doctype_force_quirks,
                    });
                }
                Some(c) => {
                    self.current_doctype_name.push(c.to_ascii_lowercase());
                }
                None => {
                    let name = std::mem::take(&mut self.current_doctype_name);
                    self.tokens.push_back(Token::Doctype {
                        name: Some(name),
                        public_id: None,
                        system_id: None,
                        force_quirks: true,
                    });
                    self.tokens.push_back(Token::Eof);
                    self.eof_emitted = true;
                }
            },

            TokenizerState::AfterDoctypeName => match ch {
                Some(c) if c.is_ascii_whitespace() => {
                    // Ignore
                }
                Some('>') => {
                    self.state = TokenizerState::Data;
                    let name = std::mem::take(&mut self.current_doctype_name);
                    self.tokens.push_back(Token::Doctype {
                        name: Some(name),
                        public_id: None,
                        system_id: None,
                        force_quirks: self.current_doctype_force_quirks,
                    });
                }
                Some(_) => {
                    // Skip remaining characters until closing `>` in initial phase
                }
                None => {
                    let name = std::mem::take(&mut self.current_doctype_name);
                    self.tokens.push_back(Token::Doctype {
                        name: Some(name),
                        public_id: None,
                        system_id: None,
                        force_quirks: true,
                    });
                    self.tokens.push_back(Token::Eof);
                    self.eof_emitted = true;
                }
            },

            TokenizerState::BogusComment => match ch {
                Some('>') => {
                    self.state = TokenizerState::Data;
                    let comment = std::mem::take(&mut self.current_comment);
                    self.tokens.push_back(Token::Comment(comment));
                }
                Some(c) => {
                    self.current_comment.push(c);
                }
                None => {
                    let comment = std::mem::take(&mut self.current_comment);
                    self.tokens.push_back(Token::Comment(comment));
                    self.tokens.push_back(Token::Eof);
                    self.eof_emitted = true;
                }
            },

            TokenizerState::RawText => match ch {
                Some('<') => {
                    let saved_pos = self.stream.pos;
                    let close_prefix = format!("/{}", self.raw_tag_name);
                    if self.stream.match_case_insensitive_str(&close_prefix) {
                        let next_ch = self.stream.next();
                        let is_tag_end = match next_ch {
                            Some('>') => true,
                            Some(c) if c == '/' || c.is_ascii_whitespace() => {
                                while let Some(c) = self.stream.next() {
                                    if c == '>' {
                                        break;
                                    }
                                }
                                true
                            }
                            None => true,
                            _ => false,
                        };

                        if is_tag_end {
                            let tag_name = std::mem::take(&mut self.raw_tag_name);
                            self.tokens.push_back(Token::EndTag { name: tag_name });
                            self.state = TokenizerState::Data;
                        } else {
                            self.stream.pos = saved_pos;
                            self.tokens.push_back(Token::Character('<'));
                        }
                    } else {
                        self.tokens.push_back(Token::Character('<'));
                    }
                }
                Some('&')
                    if self.raw_tag_name == "title" || self.raw_tag_name == "textarea" =>
                {
                    if let Some(decoded) = self.stream.try_consume_char_ref() {
                        for dc in decoded.chars() {
                            self.tokens.push_back(Token::Character(dc));
                        }
                    } else {
                        self.tokens.push_back(Token::Character('&'));
                    }
                }
                Some(c) => {
                    self.tokens.push_back(Token::Character(c));
                }
                None => {
                    self.tokens.push_back(Token::Eof);
                    self.eof_emitted = true;
                }
            },
        }
    }
}

impl Iterator for Tokenizer {
    type Item = Token;

    fn next(&mut self) -> Option<Self::Item> {
        while self.tokens.is_empty() && !self.eof_emitted {
            self.step();
        }

        self.tokens.pop_front()
    }
}
