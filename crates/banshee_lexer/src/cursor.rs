use std::str::Chars;

#[derive(Clone)]
pub struct Cursor<'a> {
    chars: Chars<'a>,
    source: &'a str,
    pos: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            chars: source.chars(),
            source,
            pos: 0,
        }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn is_eof(&self) -> bool {
        self.chars.as_str().is_empty()
    }

    pub fn first(&self) -> char {
        self.chars.clone().next().unwrap_or('\0')
    }

    pub fn second(&self) -> char {
        let mut chars = self.chars.clone();
        chars.next();
        chars.next().unwrap_or('\0')
    }

    pub fn third(&self) -> char {
        let mut chars = self.chars.clone();
        chars.next();
        chars.next();
        chars.next().unwrap_or('\0')
    }

    pub fn bump(&mut self) -> Option<char> {
        let c = self.chars.next()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    pub fn bump_while(&mut self, predicate: impl Fn(char) -> bool) {
        while !self.is_eof() && predicate(self.first()) {
            self.bump();
        }
    }

    pub fn remaining(&self) -> &'a str {
        self.chars.as_str()
    }

    pub fn starts_with(&self, s: &str) -> bool {
        self.remaining().starts_with(s)
    }

    #[allow(dead_code)]
    pub fn starts_with_ignore_case(&self, s: &str) -> bool {
        let remaining = self.remaining();
        if remaining.len() < s.len() {
            return false;
        }
        remaining[..s.len()].eq_ignore_ascii_case(s)
    }

    pub fn advance_by(&mut self, n: usize) {
        for _ in 0..n {
            self.bump();
        }
    }

    pub fn slice_from(&self, start: usize) -> &'a str {
        &self.source[start..self.pos]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_basic() {
        let mut c = Cursor::new("abc");
        assert_eq!(c.first(), 'a');
        assert_eq!(c.second(), 'b');
        assert_eq!(c.bump(), Some('a'));
        assert_eq!(c.first(), 'b');
        assert_eq!(c.pos(), 1);
    }

    #[test]
    fn cursor_unicode() {
        let mut c = Cursor::new("日本語");
        assert_eq!(c.first(), '日');
        assert_eq!(c.bump(), Some('日'));
        assert_eq!(c.pos(), 3); // UTF-8 bytes
    }
}
