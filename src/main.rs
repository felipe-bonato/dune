mod file_info;
mod key_bindings;
mod vterm;

use std::{
    cmp::min,
    env, fs,
    io::{self, stdout},
    path::{self, Path},
    process::ExitCode,
    sync::{Arc, Mutex},
    time::{self, Duration},
};

use crossterm::{
    self, cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
    execute,
    style::{self, ContentStyle, Stylize},
};

use key_bindings::{ActionCommand, ActionExplorer, ActionGlobal, KeyBindings};
use vterm::{Panel, VTerm};

const TARGET_FPS: u64 = 120;
const TARGET_FRAME_TIME_MS: u64 = 1000 / TARGET_FPS;

enum StateMsg {
    Ok,
    Info(String),
    Error(String),
}

#[derive(PartialEq, Eq)]
enum Mode {
    Explorer,
    Command,
}

struct Dune {
    pub vterm: Arc<Mutex<VTerm>>,
    should_quit: bool,
    updated_entries: bool,
    entries: Vec<file_info::FileInfo>,
    curr_dir: file_info::FileInfo,
    view_window: (u16, u16), // Start and end of entries being presented on screen
    delta_time: time::Duration,
    selected_line: u16,
    selected_entry: usize,
    state: StateMsg,
    mode: Mode,
    prompt: String,
    cursor: (u16, u16),
    key_bindings: KeyBindings,
    // Panels
    panel_header: Panel,
    panel_file_name: Panel,
    panel_file_permissions: Panel,
    panel_file_last_modified: Panel,
    panel_file_size: Panel,
    panel_state: Panel,
    panel_prompt: Panel,
}

impl Dune {
    fn new(vterm: Arc<Mutex<VTerm>>, key_bindings: KeyBindings, starting_path: path::PathBuf) -> Self {
        Self {
            vterm: vterm.clone(),
            should_quit: false,
            updated_entries: false,
            entries: Vec::new(),
            curr_dir: starting_path.try_into().expect("could not open current directory"),
            view_window: (0, 0),
            delta_time: time::Duration::ZERO,
            state: StateMsg::Ok,
            selected_line: 0,
            selected_entry: 0,
            mode: Mode::Explorer,
            prompt: "".to_owned(),
            cursor: (0, 0),
            key_bindings,
            panel_header: Panel::new(vterm.clone()),
            panel_file_name: Panel::new(vterm.clone()),
            panel_file_permissions: Panel::new(vterm.clone()),
            panel_file_last_modified: Panel::new(vterm.clone()),
            panel_file_size: Panel::new(vterm.clone()),
            panel_state: Panel::new(vterm.clone()),
            panel_prompt: Panel::new(vterm.clone()),
        }
    }

    /// Application loop
    /// Returns the path the user is currently in as Ok(path)
    pub fn run(&mut self) -> io::Result<&Path> {
        VTerm::clear()?;
        self.update_entries()?;
        self.update_panels_size();

        loop {
            let start = time::Instant::now();
            
            if self.should_quit {
                return Ok(self.curr_dir.path());
            }

            self.poll_events()?;

            let term_size = self.vterm.lock().unwrap().size();
            if term_size.0 <= 22 || term_size.1 <= 9 {
                execute!(
                    stdout(),
                    cursor::MoveTo(0, 0),
                    style::PrintStyledContent(
                        ContentStyle::new()
                            .bold()
                            .red()
                            .reverse()
                            .apply(" NO SPACE TO DRAW ")
                    ),
                    cursor::MoveTo(0, 1),
                    style::PrintStyledContent(
                        ContentStyle::new().bold().apply("Please resize window")
                    ),
                    cursor::MoveTo(0, 2),
                    style::PrintStyledContent(
                        ContentStyle::new().apply("Minimum window size: 22x9")
                    ),
                )?;
                continue;
            }

            match self.mode {
                Mode::Explorer => {
                    VTerm::cursor_hide()?;
                }

                Mode::Command => {
                    self.panel_prompt
                        .draw_text(&self.prompt, 0, 0, ContentStyle::new());
                    VTerm::cursor_show()?;
                }
            }

            if self.updated_entries {
                self.selected_line = 0;
                self.selected_entry = 0;
                self.updated_entries = false;
            }

            // Draw header
            let style = ContentStyle::new().on_grey();
            self.panel_header.fill(' ', style);
            if self.delta_time == Duration::ZERO {
                self.delta_time = Duration::from_millis(16);
            }
            let mode = match self.mode {
                Mode::Command =>  "Command Mode",
                Mode::Explorer => "Explorer Mode",
            };
            let text = format!("{path}: (total {total})", path = self.curr_dir.path_str(), total = self.entries.len());
            self.panel_header
                .draw_text(&text, 0, 0, style.bold().black());
            let w = self.vterm.lock().unwrap().width;
            self.panel_header
                .draw_text(mode, w - 1 - mode.len() as u16, 0, style.bold().black());

            // Draw entries
            let start_window = self.view_window.0 as usize; // TODO: not scroll if everything can fit on the screen.
            let end_window = min(self.view_window.1 as usize, self.entries.len());
            for (i, entry) in self.entries[start_window..end_window].iter().enumerate() {
                let i = i as u16;
                // let entry_idx = i + self.view_window.0;

                // Keeps going
                if i == self.panel_file_name.height - 1
                    && self.entries.len() > self.view_window.1 as usize
                {
                    self.panel_file_name
                        .draw_text("   ...   ", 0, i, ContentStyle::new());
                    continue;
                }

                if i == 0 && self.view_window.0 > 0 {
                    self.panel_file_name
                        .draw_text("   ...   ", 0, i, ContentStyle::new());
                    continue;
                }

                let style = if i == self.selected_line {
                    match self.mode {
                        Mode::Command => ContentStyle::new().bold().on_dark_green(),
                        Mode::Explorer => ContentStyle::new().bold().reverse(),
                    }
                } else {
                    ContentStyle::new().bold()
                };

                let mode = entry.mode();

                let style = if entry.is_dir() {
                    style.cyan()
                } else if mode & 0o001 == 1 {
                    // Is executable
                    style.green()
                } else if entry.is_read_only() {
                    style.grey()
                } else {
                    style
                };

                let style = if entry.name().starts_with('.') { // Unix hidden
                    style.dim()
                } else {
                    style
                };

                let mut name = if self.mode == Mode::Command && i == self.selected_line {
                    // Highlight selected line
                    format!(">>> {e} ", e = entry.name())
                } else {
                    format!(" {e} ", e = entry.name())
                };
                if name.len() > self.panel_file_name.width as usize {
                    name.truncate(self.panel_file_name.width.saturating_sub(3) as usize);
                    name.push_str("...");
                }
                self.panel_file_name.draw_text(&name, 0, i, style);

                self.panel_file_last_modified.draw_text(
                    entry.last_modified().format("%e %b %y").to_string().as_str(),
                    0,
                    i,
                    ContentStyle::new().dim(),
                );

                self.panel_file_size
                    .draw_text(&entry.pretty_size(), 0, i, ContentStyle::new().dim());

                let mut permissions = String::with_capacity(12); // d rwxrwxrwx
                permissions.push(if entry.is_dir() { 'd' } else { '-' });
                permissions.push(' ');
                for i in 0..3 {
                    permissions.push(if mode >> i & 0o1 > 0 { 'r' } else { '-' });
                    permissions.push(if mode >> i & 0o2 > 0 { 'w' } else { '-' });
                    permissions.push(if mode >> i & 0o4 > 0 { 'x' } else { '-' });
                }
                self.panel_file_permissions.draw_text(
                    permissions.as_str(),
                    0,
                    i,
                    ContentStyle::new().dim(),
                );
            }

            // Draw state
            let (text, style) = match &self.state {
                StateMsg::Error(msg) => (
                    format!("ERROR: {msg}."),
                    ContentStyle::new().on_dark_red().white().bold(),
                ),
                StateMsg::Ok => (
                    // "TODO: Add some info here".to_string(),
                    format!(
                        "fps: {fps} window: {window} content_len: {cl} selected_line: {sl} selected_entry: {se} view_window: {vws}..{vwe} ({vwl}) panel_state: {ps} ",
                        fps = Duration::from_secs(1).as_micros() / self.delta_time.as_micros(),
                        sl = self.selected_line,
                        se = self.selected_entry,
                        vws = self.view_window.0,
                        vwe = self.view_window.1,
                        window = self.panel_file_name.height,
                        cl = self.entries.len(),
                        vwl = self.view_window.1 - self.view_window.0,
                        ps = self.panel_state.width
                    ),
                    ContentStyle::new().on_white().black(),
                ),
                StateMsg::Info(msg) => (
                    msg.to_owned(),
                    ContentStyle::new().on_white().black().bold(),
                ),
            };
            self.panel_state.fill(' ', style);
            self.panel_state.draw_text(&text, 0, 0, style);

            self.render_terminal()?;

            // Cursor
            self.vterm
                .lock()
                .unwrap()
                .cursor_move(self.cursor.0, self.cursor.1)?;

            self.delta_time = time::Instant::now() - start;
        }
    }

    fn view_window_overflow(&self, i: u16) -> bool {
        i >= self.view_window.1 - 1 && i <= self.entries.len() as u16
    }

    fn view_window_underflow(&self, i: u16) -> bool {
        self.view_window.0 > 0 && i == self.view_window.0
    }

    fn update_panels_size(&mut self) {
        let w = self.vterm.lock().unwrap().width;
        let h = self.vterm.lock().unwrap().height;

        if w < 4 || h < 3 {
            // Not enough space to draw anything
            return;
        }

        self.panel_header.update_size(0, 0, w, 1);

        {
            const PERMISSIONS_LEN: u16 = 12;
            const SIZE_LEN: u16 = 8;
            const LAST_MODIFIED_LEN: u16 = 10;
            let mut len_left: u16 = w; // Lenght of the fixed elements on the table

            len_left = len_left.saturating_sub(PERMISSIONS_LEN);
            self.panel_file_permissions
                .update_size(len_left, 1, PERMISSIONS_LEN, h - 3);

            len_left = len_left.saturating_sub(SIZE_LEN);
            self.panel_file_size
                .update_size(len_left, 1, SIZE_LEN, h - 3);

            len_left = len_left.saturating_sub(LAST_MODIFIED_LEN);
            self.panel_file_last_modified
                .update_size(len_left, 1, LAST_MODIFIED_LEN, h - 3);

            self.panel_file_name.update_size(0, 1, len_left, h - 3);
        }

        self.panel_state.update_size(0, h - 2, w, 1);
        self.panel_prompt.update_size(0, h - 1, w, 1);

        self.resize_view_window();
    }

    fn update_entries(&mut self) -> io::Result<()> {
        // Other entries
        let curr_dir = env::current_dir()?;

        self.entries.clear();
        for entry in fs::read_dir(&curr_dir)? {
            self.entries.push(entry?.try_into()?);
        }
        self.resize_view_window();

        self.curr_dir = curr_dir.try_into()?;

        self.updated_entries = true;

        Ok(())
    }

    fn resize_view_window(&mut self) {
        self.view_window = (
            0,
            min(self.entries.len() as u16, self.panel_file_name.height),
        );
        // TODO: Move this out of here
    }

    fn render_terminal(&mut self) -> io::Result<()> {
        self.vterm.lock().unwrap().flush()?;
        self.vterm.lock().unwrap().cursor_move(0, 1)
    }

    fn poll_events(&mut self) -> io::Result<()> {
        loop {
            match event::poll(Duration::from_millis(TARGET_FRAME_TIME_MS)) {
                Ok(true) => {
                    self.handle_event(event::read()?)?;
                    continue;
                }
                Ok(false) => return Ok(()),
                Err(e) => return Err(e),
            }
        }
    }

    fn handle_event(&mut self, evt: Event) -> io::Result<()> {
        // Special events
        if let Event::Resize(w, h) = evt {
            self.vterm.lock().unwrap().width = w;
            self.vterm.lock().unwrap().height = h;
            self.vterm.lock().unwrap().queue_empty();
            VTerm::clear()?;
            self.update_panels_size();
            return Ok(());
        }

        if let Some(action) = self.key_bindings.get_global(&evt) {
            match action {
                ActionGlobal::Quit => {
                    self.should_quit = true;
                }
                ActionGlobal::ModeChange => {
                    // Toggle mode
                    self.mode = if self.mode == Mode::Explorer {
                        self.cursor = (0, self.vterm.lock().unwrap().height - 1);
                        self.state = StateMsg::Info("Command:".into());
                        Mode::Command
                    } else {
                        self.state = StateMsg::Ok;
                        Mode::Explorer
                    };
                }
            }
        }

        match self.mode {
            Mode::Command => {
                if let Some(action) = self.key_bindings.get_command(&evt) {
                    // If known command
                    match action {
                        ActionCommand::Execute => {
                            // TODO: This require better input handling
                            let mut prompt_split = self.prompt.split(' ');
                            if let Some(cmd) = prompt_split.next() {
                                let args = prompt_split.collect::<Vec<&str>>();
                                let mut exec = std::process::Command::new(cmd);
                                let exec = exec
                                    .args(args)
                                    .arg(self.entries[self.selected_entry].path_str());
                                // exec.spawn()?; // Let's not spawn until we have better input handling
                                self.state = StateMsg::Info(format!("Execute: {exec:?}"));
                                self.update_entries()?;
                            }
                        }

                        ActionCommand::PromptBackspace => {
                            self.prompt.pop();
                            self.cursor.0 -= 1;
                        }
                    }
                } else {
                    // It's just a char
                    match evt {
                        Event::Key(KeyEvent {
                            code: KeyCode::Char(ch),
                            kind: KeyEventKind::Press,
                            ..
                        }) => {
                            self.prompt.push(ch);
                            self.cursor.0 += 1;
                        }
                        _ => self.unknown_event(evt),
                    }
                }
            }
            Mode::Explorer => {
                if let Some(action) = self.key_bindings.get_explorer(&evt) {
                    match action {
                        ActionExplorer::ScrollUp => {
                            if self.selected_entry > 0 {
                                self.selected_entry -= 1;
                                if self.view_window_underflow(
                                    self.selected_entry.saturating_sub(1) as u16
                                ) {
                                    self.view_window.0 = self.view_window.0.saturating_sub(1);
                                    self.view_window.1 = self.view_window.1.saturating_sub(1);
                                } else {
                                    self.selected_line = self.selected_line.saturating_sub(1);
                                }
                            }
                        }

                        ActionExplorer::ScrollDown => {
                            if !self.entries.is_empty()
                                && self.selected_entry < self.entries.len() - 1
                            {
                                self.selected_entry += 1;
                                if self.entries.len() > self.panel_file_name.height as usize // Don't need to scroll if everything fits.
                                            && self.view_window_overflow(self.selected_entry as u16 + 1)
                                {
                                    self.view_window.0 += 1;
                                    self.view_window.1 += 1;
                                } else if self.selected_entry < self.entries.len() {
                                    self.selected_line += 1;
                                }
                            }
                        }

                        ActionExplorer::DirEnter => {
                            if let Some(entry) = self.entries.get(self.selected_entry) {
                                if !entry.is_dir() {
                                    self.state = StateMsg::Error(format!(
                                        "Tried to enter `{f}`, but failed because it is not a directory",
                                        f = entry.name()
                                    ));
                                } else if let Err(err) = cd(entry.name()) {
                                    self.state = StateMsg::Error(format!(
                                        "Tried to enter `{f}`, but failed because {err}",
                                        f = entry.name()
                                    ))
                                } else {
                                    self.update_entries()?;
                                }
                            } else {
                                unreachable!("Selected line is out of bounds");
                            }
                            // TODO: handle errors (file is not dir, no permissions...), print then on status bar?
                        }

                        ActionExplorer::DirLeave => {
                            cd("..")?;
                            self.update_entries()?;
                        }

                        ActionExplorer::EntriesUpdate => self.update_entries()?,
                    }
                }
            }
        }

        Ok(())
    }

    fn unknown_event(&mut self, _evt: Event) {
        // For now, don't do anything...
    }
}

fn cd<P: AsRef<Path>>(dir: P) -> io::Result<()> {
    env::set_current_dir(dir)
}

fn main() -> ExitCode {
    let starting_dir = env::current_dir().unwrap_or_else(|e| {
        eprintln!("ERROR: {e:?}");
        ".".into() // Default to `.` as last choice
    });

    let mut app = Dune::new(Arc::new(Mutex::new(VTerm::new())), key_bindings::new(), starting_dir);

    let path = match app.run() {
        Err(e) => {
            eprintln!("ERROR: {e:?}");
            return ExitCode::FAILURE;
        }
        Ok(path) => {
            path
        }
    };
    
    println!("Run: cd {path:?}");
    
    ExitCode::SUCCESS
}
