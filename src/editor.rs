pub struct Editor {
    pub lines: Vec<String>,
    pub row: usize,
    pub col: usize,
}

impl Editor {
    pub fn new() -> Self {
        Self { lines: vec![String::new()], row: 0, col: 0 }
    }

    pub fn from_text(text: String) -> Self {
        let mut lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        Self { lines, row: 0, col: 0 }
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn insert_char(&mut self, c: char) {
        self.lines[self.row].insert(self.col, c);
        self.col += 1;
    }

    pub fn newline(&mut self) {
        let rest = self.lines[self.row].split_off(self.col);
        self.lines.insert(self.row + 1, rest);
        self.row += 1;
        self.col = 0;
    }

    pub fn backspace(&mut self) {
        if self.col > 0 {
            self.col -= 1;
            self.lines[self.row].remove(self.col);
        } else if self.row > 0 {
            let cur = self.lines.remove(self.row);
            self.row -= 1;
            self.col = self.lines[self.row].len();
            self.lines[self.row].push_str(&cur);
        }
    }

    pub fn left(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = self.lines[self.row].len();
        }
    }

    pub fn right(&mut self) {
        if self.col < self.lines[self.row].len() {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
    }

    pub fn up(&mut self) {
        if self.row > 0 {
            self.row -= 1;
            self.col = self.col.min(self.lines[self.row].len());
        }
    }

    pub fn down(&mut self) {
        if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = self.col.min(self.lines[self.row].len());
        }
    }

    pub fn home(&mut self) {
        self.col = 0;
    }

    pub fn end(&mut self) {
        self.col = self.lines[self.row].len();
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_types_deletes_and_splits() {
        let mut e = Editor::new();
        for c in "select 1".chars() {
            e.insert_char(c);
        }
        assert_eq!(e.text(), "select 1");
        assert_eq!((e.row, e.col), (0, 8));

        e.backspace();
        assert_eq!(e.text(), "select ");

        e.end();
        e.newline();
        assert_eq!(e.text(), "select \n");
        assert_eq!((e.row, e.col), (1, 0));

        e.insert_char('x');
        e.up();
        e.end();
        assert_eq!(e.col, 7);
        e.down();
        assert_eq!((e.row, e.col), (1, 1));
    }

    #[test]
    fn editor_from_text_roundtrips() {
        let e = Editor::from_text("a\nb\nc".into());
        assert_eq!(e.text(), "a\nb\nc");
        assert_eq!(e.lines.len(), 3);
    }
}