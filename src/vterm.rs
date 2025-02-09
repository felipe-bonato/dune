use std::{io::{self, stdout, Write}, sync::{Arc, Mutex}};

use crossterm::{
    cursor, execute, queue,
    style::{self, ContentStyle},
    terminal::{self, ClearType},
};

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Cell {
    ch: char,
    style: ContentStyle,
}

impl Cell {
    fn new() -> Self {
        Self {
            ch: ' ',
            style: ContentStyle::new(),
        }
    }
}

pub struct VTerm {
    vterminal_last: Vec<Cell>,
    vterminal: Vec<Cell>,
    pub width: u16,
    pub height: u16,
}

impl VTerm {
    pub fn new() -> Self {
        terminal::enable_raw_mode().expect("could not enable raw mode");
        let (w, h) = terminal::size().expect("could not get terminal size");
        queue!(stdout(), cursor::MoveTo(0, 0)).expect("could not move cursor");
        queue!(stdout(), cursor::Hide).expect("could not hide cursor");
        VTerm {
            vterminal_last: Self::new_empty_vterminal(w, h),
            vterminal: Self::new_empty_vterminal(w, h),
            width: w,
            height: h,
        }
    }

    /// Immediately clears the terminal.
    /// Doesn't affect the queued commands.
    /// Useful for resizes.
    pub fn clear() -> io::Result<()> {
        execute!(stdout(), terminal::Clear(ClearType::All))
    }

    /// Immediately hides the cursor.
    pub fn cursor_hide() -> io::Result<()> {
        queue!(stdout(), cursor::Hide)
    }

    /// Immediately shows the cursor.
    pub fn cursor_show() -> io::Result<()> {
        queue!(stdout(), cursor::Show)
    }

    /// Immediately moves the cursor to a new position.
    pub fn cursor_move(&mut self, x: u16, y: u16) -> io::Result<()> {
        queue!(stdout(), cursor::MoveTo(x, y))
    }

    /// Gets the terminal size
    pub fn size(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    /// Queues a character into the vterminal.
    pub fn queue_char(&mut self, ch: char, x: u16, y: u16, style: ContentStyle) {
        let i = self.index(x, y);
        self.vterminal[i] = Cell { ch, style };
    }

    /// Queues a string into the vterminal.
    pub fn queue_text(&mut self, text: &str, x: u16, y: u16, style: ContentStyle) {
        for (i, c) in text.chars().enumerate() {
            let x_offset = x as usize + i;
            if x_offset > self.width as usize {
                panic!("Write x outside of bounds! You dummy!");
            }

            if y > self.height {
                panic!("Write y outside of bounds! You dummy!");
            }

            self.queue_char(c, x_offset as u16, y, style);
        }
    }

    /// Empties everything queued into the vterminal
    pub fn queue_empty(&mut self) {
        self.vterminal_last = Self::new_empty_vterminal(self.width, self.height);
        self.vterminal = Self::new_empty_vterminal(self.width, self.height);
    }

    /// Flushes the vterminal to the screen.
    pub fn flush(&mut self) -> io::Result<()> {
        for i in 0..self.width as usize * self.height as usize {
            if self.vterminal[i] != self.vterminal_last[i] {
                let x = (i % self.width as usize) as u16;
                let y = (i / self.width as usize) as u16;

                queue!(
                    stdout(),
                    cursor::MoveTo(x, y),
                    style::PrintStyledContent(self.vterminal[i].style.apply(self.vterminal[i].ch)),
                )?;
            }
        }

        self.vterminal_last = self.vterminal.clone(); // TODO: Do this without copying?
        self.vterminal = Self::new_empty_vterminal(self.width, self.height);

        stdout().flush()?;

        Ok(())
    }

    fn new_empty_vterminal(width: u16, height: u16) -> Vec<Cell> {
        vec![Cell::new(); width as usize * height as usize]
    }

    fn index(&self, x: u16, y: u16) -> usize {
        x as usize + y as usize * self.width as usize
    }
}

impl Drop for VTerm {
    fn drop(&mut self) {
        let _ = execute!(stdout(), terminal::Clear(ClearType::All), cursor::Show);
        crossterm::terminal::disable_raw_mode().expect("could not disable raw mode");
    }
}

pub struct Panel {
    vterm: Arc<Mutex<VTerm>>,
    x: u16,
    y: u16,
    pub width: u16,
    pub height: u16,
}

impl Panel {
    pub fn new(vterm: Arc<Mutex<VTerm>>) -> Self {
        Self {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            vterm,
        }
    }

    pub fn draw_text(&mut self, text: &str, x: u16, y: u16, style: ContentStyle) {
        if x > self.width || y > self.height {
            panic!("Out of panel bounds");
        }

        if text.len() > self.width as usize {
            self.vterm.lock().unwrap().queue_text(
                text.chars()
                    .take(self.width as usize)
                    .collect::<String>()
                    .as_str(),
                self.x + x,
                self.y + y,
                style,
            );
        } else {
            self.vterm
                .lock()
                .unwrap()
                .queue_text(text, self.x + x, self.y + y, style);
        }
    }

    pub fn update_size(&mut self, x: u16, y: u16, width: u16, height: u16) {
        self.x = x;
        self.y = y;
        self.width = width;
        self.height = height;
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.fill(' ', ContentStyle::new())
    }

    pub fn fill(&mut self, ch: char, style: ContentStyle) {
        for x in self.x..self.x + self.width {
            for y in self.y..self.y + self.height {
                self.vterm.lock().unwrap().queue_char(ch, x, y, style);
            }
        }
    }
}
