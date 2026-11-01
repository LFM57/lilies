#![deny(unsafe_code)]

use crate::ast::{Combinator, Declaration, Rule, Selector, StyleSheet, Unit, Value};
use crate::color::Color;
use crate::error::CssError;

/// Parser for CSS stylesheets conforming to the Milestone 2 requirements.
pub struct CssParser {
    chars: Vec<char>,
    pos: usize,
}

impl CssParser {
    pub fn new(input: &str) -> Self {
        Self {
            chars: input.chars().collect(),
            pos: 0,
        }
    }

    /// Parses the entire input into a `StyleSheet`.
    pub fn parse_stylesheet(&mut self) -> Result<StyleSheet, CssError> {
        let mut stylesheet = StyleSheet::new();

        loop {
            self.skip_whitespace_and_comments();
            if self.is_eof() {
                break;
            }

            match self.parse_rule() {
                Ok(rule) => {
                    stylesheet.add_rule(rule);
                }
                Err(_) => {
                    self.skip_to_rule_end();
                }
            }
        }

        Ok(stylesheet)
    }

    /// Skips forward to the end of an invalid rule or statement at-rule (`;` or matching `{ ... }`).
    fn skip_to_rule_end(&mut self) {
        let mut in_quote: Option<char> = None;
        while let Some(c) = self.peek() {
            self.pos += 1;
            if let Some(q) = in_quote {
                if c == q {
                    in_quote = None;
                }
                continue;
            }
            if c == '"' || c == '\'' {
                in_quote = Some(c);
            } else if c == ';' {
                // Statement at-rule such as @charset or @import ends at ';'
                return;
            } else if c == '{' {
                break;
            }
        }
        let mut depth = 1;
        while let Some(c) = self.peek() {
            self.pos += 1;
            if let Some(q) = in_quote {
                if c == q {
                    in_quote = None;
                }
                continue;
            }
            if c == '"' || c == '\'' {
                in_quote = Some(c);
            } else if c == '{' {
                depth += 1;
            } else if c == '}' {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
        }
    }

    /// Skips forward to the end of an invalid declaration by stopping at ';' or '}'.
    fn skip_to_declaration_end(&mut self) {
        let mut paren_depth = 0;
        while let Some(c) = self.peek() {
            match c {
                '(' => paren_depth += 1,
                ')' if paren_depth > 0 => paren_depth -= 1,
                ';' if paren_depth == 0 => break,
                '}' if paren_depth == 0 => break,
                _ => {}
            }
            self.pos += 1;
        }
    }

    /// Parses a sequence of semicolon-separated declarations (e.g. from an HTML `style` attribute).
    pub fn parse_declarations(&mut self) -> Result<Vec<Declaration>, CssError> {
        let mut declarations = Vec::new();
        loop {
            self.skip_whitespace_and_comments();
            if self.is_eof() {
                break;
            }
            if self.peek() == Some(';') {
                self.pos += 1;
                continue;
            }

            match self.parse_declaration() {
                Ok(declaration) => {
                    declarations.push(declaration);
                }
                Err(_) => {
                    self.skip_to_declaration_end();
                }
            }

            self.skip_whitespace_and_comments();
            if self.peek() == Some(';') {
                self.pos += 1;
            } else if self.is_eof() {
                break;
            } else {
                self.skip_to_declaration_end();
                if matches!(self.peek(), Some(';' | '}')) {
                    self.pos += 1;
                }
            }
        }
        Ok(declarations)
    }

    /// Parses a single rule: `selectors { declarations }`.
    fn parse_rule(&mut self) -> Result<Rule, CssError> {
        let selectors = self.parse_selectors()?;
        if selectors.is_empty() {
            return Err(CssError::ParseError(
                "Expected at least one selector".into(),
            ));
        }

        self.skip_whitespace_and_comments();
        self.consume_char('{')?;

        let mut declarations = Vec::new();
        loop {
            self.skip_whitespace_and_comments();
            if self.peek() == Some('}') {
                self.pos += 1;
                break;
            }
            if self.is_eof() {
                return Err(CssError::UnexpectedEof);
            }

            match self.parse_declaration() {
                Ok(declaration) => {
                    declarations.push(declaration);
                }
                Err(_) => {
                    self.skip_to_declaration_end();
                }
            }

            self.skip_whitespace_and_comments();
            if self.peek() == Some(';') {
                self.pos += 1;
            } else if self.peek() == Some('}') {
                // Trailing declaration without semicolon is valid CSS
            } else if !self.is_eof() {
                self.skip_to_declaration_end();
                if self.peek() == Some(';') {
                    self.pos += 1;
                }
            }
        }

        Ok(Rule::new(selectors, declarations))
    }

    /// Parses comma-separated selectors, supporting descendant/child combinators and pseudo-classes.
    fn parse_selectors(&mut self) -> Result<Vec<Selector>, CssError> {
        let mut selectors = Vec::new();

        loop {
            self.skip_whitespace_and_comments();
            if self.is_eof() || self.peek() == Some('{') {
                break;
            }

            // If a rule starts with '@', fail immediately so skip_to_rule_end handles the at-rule
            if selectors.is_empty() && self.peek() == Some('@') {
                return Err(CssError::ParseError("At-rule not supported as style rule".into()));
            }

            match self.parse_complex_selector() {
                Ok(selector) => {
                    selectors.push(selector);
                }
                Err(_) => {
                    // Skip to the next ',' or '{' so one unsupported selector doesn't drop valid sibling selectors
                    while let Some(c) = self.peek() {
                        if c == ',' || c == '{' || c == ';' {
                            break;
                        }
                        self.pos += 1;
                    }
                    if self.peek() == Some(';') {
                        return Err(CssError::ParseError("Invalid selector before ';'".into()));
                    }
                }
            }

            self.skip_whitespace_and_comments();
            if self.peek() == Some(',') {
                self.pos += 1;
            } else {
                break;
            }
        }

        Ok(selectors)
    }

    /// Parses a complex selector consisting of one or more compound selectors joined by combinators.
    fn parse_complex_selector(&mut self) -> Result<Selector, CssError> {
        let mut current = self.parse_compound_selector()?;

        loop {
            let pos_before_ws = self.pos;
            self.skip_whitespace_and_comments();
            let had_ws = self.pos > pos_before_ws;

            match self.peek() {
                None | Some(',') | Some('{') => break,
                Some('>') => {
                    self.pos += 1;
                    self.skip_whitespace_and_comments();
                    let mut next = self.parse_compound_selector()?;
                    next.parent = Some((Combinator::Child, Box::new(current)));
                    current = next;
                }
                Some(_) if had_ws => {
                    let mut next = self.parse_compound_selector()?;
                    next.parent = Some((Combinator::Descendant, Box::new(current)));
                    current = next;
                }
                Some(ch) => {
                    return Err(CssError::ParseError(format!(
                        "Unexpected character '{}' in selector at position {}",
                        ch, self.pos
                    )));
                }
            }
        }

        Ok(current)
    }

    /// Parses a single compound selector (e.g. `div`, `.box`, `#main`, `div.box`, `a:hover`, `*`).
    fn parse_compound_selector(&mut self) -> Result<Selector, CssError> {
        let mut tag_name = None;
        let mut id = None;
        let mut classes = Vec::new();
        let mut pseudo_classes = Vec::new();
        let mut has_universal = false;

        if let Some(ch) = self.peek() {
            if ch == '*' {
                self.pos += 1;
                has_universal = true;
            } else if is_ident_start(ch) {
                tag_name = Some(self.parse_identifier()?);
            }
        }

        // Parse immediately chained .class, #id, or :pseudo-class without intervening whitespace
        loop {
            match self.peek() {
                Some('.') => {
                    self.pos += 1;
                    let class_name = self.parse_identifier()?;
                    classes.push(class_name);
                }
                Some('#') => {
                    self.pos += 1;
                    let id_name = self.parse_identifier()?;
                    id = Some(id_name);
                }
                Some(':') => {
                    self.pos += 1;
                    let is_pseudo_element = if self.peek() == Some(':') {
                        self.pos += 1;
                        true
                    } else {
                        false
                    };
                    let name = self.parse_identifier()?.to_ascii_lowercase();
                    if self.peek() == Some('(') {
                        self.pos += 1;
                        let mut depth = 1;
                        while let Some(c) = self.peek() {
                            self.pos += 1;
                            if c == '(' {
                                depth += 1;
                            } else if c == ')' {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                        }
                    }
                    if is_pseudo_element {
                        pseudo_classes.push(format!("::{}", name));
                    } else {
                        pseudo_classes.push(name);
                    }
                }
                _ => break,
            }
        }

        if tag_name.is_none()
            && id.is_none()
            && classes.is_empty()
            && pseudo_classes.is_empty()
            && !has_universal
        {
            return Err(CssError::ParseError(format!(
                "Invalid selector at position {}",
                self.pos
            )));
        }

        Ok(Selector {
            tag_name,
            id,
            classes,
            pseudo_classes,
            parent: None,
        })
    }

    /// Parses a property declaration: `name: value [!important]`.
    fn parse_declaration(&mut self) -> Result<Declaration, CssError> {
        let name = self.parse_identifier()?;

        self.skip_whitespace_and_comments();
        self.consume_char(':')?;
        self.skip_whitespace_and_comments();

        let mut values = Vec::new();
        let mut important = false;
        while let Some(ch) = self.peek() {
            if ch == ';' || ch == '}' {
                break;
            }
            if ch == ',' || ch == '/' {
                self.pos += 1;
                self.skip_whitespace_and_comments();
                continue;
            }
            if ch == '!' {
                self.pos += 1;
                self.skip_whitespace_and_comments();
                let ident = self.parse_identifier()?;
                if !ident.eq_ignore_ascii_case("important") {
                    return Err(CssError::ParseError(format!(
                        "Expected 'important' after '!', found '{}'",
                        ident
                    )));
                }
                important = true;
                self.skip_whitespace_and_comments();
                continue;
            }
            let val = self.parse_single_value()?;
            values.push(val);
            self.skip_whitespace_and_comments();
        }

        let value = match values.len() {
            0 => {
                return Err(CssError::ParseError(format!(
                    "Declaration for '{}' has no value",
                    name
                )))
            }
            1 => values.remove(0),
            _ => Value::List(values),
        };

        Ok(Declaration::new_important(name, value, important))
    }

    /// Parses a single value token: hex color, length/dimension, keyword, or named color.
    fn parse_single_value(&mut self) -> Result<Value, CssError> {
        self.skip_whitespace_and_comments();

        let ch = self.peek().ok_or(CssError::UnexpectedEof)?;

        if ch == '#' {
            // Hex color
            self.pos += 1;
            let start = self.pos;
            while let Some(c) = self.peek() {
                if c.is_ascii_hexdigit() {
                    self.pos += 1;
                } else {
                    break;
                }
            }
            let hex_str: String = self.chars[start..self.pos].iter().collect();
            let color = Color::from_hex(&hex_str).ok_or_else(|| {
                CssError::ParseError(format!("Invalid hex color: '#{}'", hex_str))
            })?;
            return Ok(Value::Color(color));
        }

        if ch.is_ascii_digit() || ch == '-' || ch == '+' || ch == '.' {
            // Check if this is indeed a number
            let start = self.pos;
            if ch == '-' || ch == '+' {
                self.pos += 1;
            }
            let mut has_digit = false;
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    has_digit = true;
                    self.pos += 1;
                } else {
                    break;
                }
            }
            if self.peek() == Some('.') {
                self.pos += 1;
                while let Some(c) = self.peek() {
                    if c.is_ascii_digit() {
                        has_digit = true;
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
            }

            if has_digit {
                let num_str: String = self.chars[start..self.pos].iter().collect();
                let num: f32 = num_str.parse().map_err(|_| {
                    CssError::ParseError(format!("Invalid numeric value: '{}'", num_str))
                })?;

                // Check for unit (e.g. px, em, rem, vh, vw, pt, %)
                if let Some(c) = self.peek() {
                    if is_ident_start(c) || c == '%' {
                        let unit_str = if c == '%' {
                            self.pos += 1;
                            "%".to_string()
                        } else {
                            self.parse_identifier()?
                        };
                        match unit_str.to_ascii_lowercase().as_str() {
                            "px" => return Ok(Value::Length(num, Unit::Px)),
                            "em" => return Ok(Value::Length(num, Unit::Em)),
                            "rem" => return Ok(Value::Length(num, Unit::Rem)),
                            "vh" => return Ok(Value::Length(num * 6.0, Unit::Px)),
                            "vw" => return Ok(Value::Length(num * 8.0, Unit::Px)),
                            "pt" => return Ok(Value::Length(num * (96.0 / 72.0), Unit::Px)),
                            "%" => return Ok(Value::Keyword(format!("{}%", num))),
                            _ => return Ok(Value::Keyword(format!("{}{}", num, unit_str))),
                        }
                    }
                }

                // If zero without unit, or plain number, default to Px for length
                return Ok(Value::Length(num, Unit::Px));
            } else {
                // Rewind if it was just a '-' not followed by a digit
                self.pos = start;
            }
        }

        if is_ident_start(ch) {
            let ident = self.parse_identifier()?;

            // Check for functional notation: rgb(...) or rgba(...)
            if (ident.eq_ignore_ascii_case("rgb") || ident.eq_ignore_ascii_case("rgba"))
                && self.peek() == Some('(')
            {
                return self.parse_rgb_function(&ident);
            }

            // Check for generic function: e.g. light-dark(...), calc(...), var(...)
            if self.peek() == Some('(') {
                return self.parse_generic_function(&ident);
            }

            // Check if it's a recognized named color
            if let Some(color) = Color::from_name(&ident) {
                return Ok(Value::Color(color));
            }

            return Ok(Value::Keyword(ident));
        }

        // Quoted string
        if ch == '"' || ch == '\'' {
            let quote = ch;
            self.pos += 1;
            let start = self.pos;
            while let Some(c) = self.peek() {
                if c == quote {
                    let s: String = self.chars[start..self.pos].iter().collect();
                    self.pos += 1;
                    return Ok(Value::Keyword(s));
                }
                self.pos += 1;
            }
            return Err(CssError::UnexpectedEof);
        }

        Err(CssError::ParseError(format!(
            "Unexpected character '{}' at position {}",
            ch, self.pos
        )))
    }

    /// Parses arbitrary functional notation: `fn_name(content...)`.
    fn parse_generic_function(&mut self, fn_name: &str) -> Result<Value, CssError> {
        self.consume_char('(')?;
        let mut depth = 1;
        let mut content = String::new();
        while let Some(c) = self.peek() {
            self.pos += 1;
            if c == '(' {
                depth += 1;
            } else if c == ')' {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            content.push(c);
        }
        if depth > 0 {
            return Err(CssError::UnexpectedEof);
        }

        // Support light-dark(light_color, dark_color) by extracting light color
        if fn_name.eq_ignore_ascii_case("light-dark") {
            let parts: Vec<&str> = content.split(',').map(|s| s.trim()).collect();
            if let Some(&first) = parts.first() {
                if let Some(color) = Color::from_hex(first).or_else(|| Color::from_name(first)) {
                    return Ok(Value::Color(color));
                }
            }
        }

        Ok(Value::Keyword(format!("{}({})", fn_name, content)))
    }

    /// Parses functional color notation: `rgb(r, g, b)` or `rgba(r, g, b, a)`.
    fn parse_rgb_function(&mut self, fn_name: &str) -> Result<Value, CssError> {
        self.consume_char('(')?;
        self.skip_whitespace_and_comments();

        let r = self.parse_u8_component()?;
        self.skip_whitespace_and_comments();
        self.consume_char(',')?;
        self.skip_whitespace_and_comments();

        let g = self.parse_u8_component()?;
        self.skip_whitespace_and_comments();
        self.consume_char(',')?;
        self.skip_whitespace_and_comments();

        let b = self.parse_u8_component()?;
        self.skip_whitespace_and_comments();

        let a = if fn_name.eq_ignore_ascii_case("rgba") && self.peek() == Some(',') {
            self.consume_char(',')?;
            self.skip_whitespace_and_comments();
            // Alpha could be float 0.0-1.0 or u8
            let start = self.pos;
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() || c == '.' {
                    self.pos += 1;
                } else {
                    break;
                }
            }
            let alpha_str: String = self.chars[start..self.pos].iter().collect();
            let alpha_f: f32 = alpha_str.parse().map_err(|_| {
                CssError::ParseError(format!("Invalid alpha in rgba(): '{}'", alpha_str))
            })?;
            if alpha_f <= 1.0 {
                (alpha_f * 255.0).round() as u8
            } else {
                alpha_f.min(255.0) as u8
            }
        } else {
            255
        };

        self.skip_whitespace_and_comments();
        self.consume_char(')')?;

        Ok(Value::Color(Color::rgba(r, g, b, a)))
    }

    fn parse_u8_component(&mut self) -> Result<u8, CssError> {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.pos += 1;
            } else {
                break;
            }
        }
        let s: String = self.chars[start..self.pos].iter().collect();
        s.parse::<u8>()
            .map_err(|_| CssError::ParseError(format!("Invalid color component: '{}'", s)))
    }

    /// Parses a CSS identifier: `[a-zA-Z_-][a-zA-Z0-9_-]*`.
    fn parse_identifier(&mut self) -> Result<String, CssError> {
        let start = self.pos;
        let Some(first) = self.peek() else {
            return Err(CssError::UnexpectedEof);
        };
        if !is_ident_start(first) {
            return Err(CssError::ParseError(format!(
                "Expected identifier start, found '{}'",
                first
            )));
        }
        self.pos += 1;

        while let Some(c) = self.peek() {
            if is_ident_char(c) {
                self.pos += 1;
            } else {
                break;
            }
        }

        Ok(self.chars[start..self.pos].iter().collect())
    }

    /// Skips all whitespace and `/* ... */` comments.
    pub fn skip_whitespace_and_comments(&mut self) {
        loop {
            // Whitespace
            while let Some(c) = self.peek() {
                if c.is_ascii_whitespace() {
                    self.pos += 1;
                } else {
                    break;
                }
            }

            // Comments
            if self.peek() == Some('/') && self.chars.get(self.pos + 1) == Some(&'*') {
                self.pos += 2; // skip /*
                while self.pos < self.chars.len() {
                    if self.chars[self.pos] == '*' && self.chars.get(self.pos + 1) == Some(&'/') {
                        self.pos += 2; // skip */
                        break;
                    }
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn consume_char(&mut self, expected: char) -> Result<(), CssError> {
        match self.peek() {
            Some(c) if c == expected => {
                self.pos += 1;
                Ok(())
            }
            Some(c) => Err(CssError::ParseError(format!(
                "Expected '{}', found '{}'",
                expected, c
            ))),
            None => Err(CssError::UnexpectedEof),
        }
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.chars.len()
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c == '-'
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// Convenience function to parse a CSS string into a `StyleSheet`.
pub fn parse_stylesheet(css: &str) -> Result<StyleSheet, CssError> {
    let mut parser = CssParser::new(css);
    parser.parse_stylesheet()
}

/// Convenience function to parse semicolon-separated CSS declarations (e.g. from an HTML `style` attribute).
pub fn parse_declarations(css: &str) -> Result<Vec<Declaration>, CssError> {
    let mut parser = CssParser::new(css);
    parser.parse_declarations()
}
