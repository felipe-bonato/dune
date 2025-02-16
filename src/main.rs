mod file_info;
mod key_bindings;
mod vec2;
mod vterm;

use std::{
    cmp::min,
    env, fs, io, ops, path, process, str,
    sync::{Arc, Mutex},
    time,
};

use crossterm::{
    cursor,
    event,
    execute,
    style,
    // Keep qualified for now due to traits
    style::Stylize,
};

use key_bindings::{ActionCommand, ActionExplorer, ActionGlobal, KeyBindings};
use vec2::Vec2;
use vterm::{Panel, VTerm};

// TODO: Better separation into modules.
// TODO: Improve text styling, currently it's all over the place
//       it should come from a config file (maybe the same that the keybindings use?).

const DEBUG_MODE: bool = true;

fn sat_add(value: usize, add: usize, saturation: usize) -> usize {
    // TODO: This break if saturates usize, but because we are using only for u16 it's fine.
    if value + add >= saturation {
        saturation
    } else {
        value + add
    }
}

fn sat_sub(value: usize, sub: usize, saturation: usize) -> usize {
    value
        .checked_sub(sub)
        .map(|v| if v < saturation { saturation } else { v })
        .unwrap_or(saturation)
}

/// Saturates as if u16
fn sat_inc(value: usize, saturation: usize) -> usize {
    if value >= saturation {
        saturation
    } else {
        value + 1
    }
}

/// Saturates as if u16
fn sat_dec(value: usize, saturation: usize) -> usize {
    if value <= saturation {
        saturation
    } else {
        value - 1
    }
}

#[derive(Debug)]
struct ScrollingWindow {
    viewport: Vec2, // Start and end of visible items.
    entries_len: usize,
    window_len: usize, // The height of the panel that the entries will be rendered to
    selected_entry: usize, // Index of the selected entry in the self.entries slice.
    selected_line: usize, // Index of the selected line in the viewport.
}

impl ScrollingWindow {
    fn new(entries_len: usize, screen_len: usize) -> Self {
        let mut s = Self {
            entries_len,
            window_len: 0,
            selected_entry: 0,
            selected_line: 0,
            viewport: vec2::ZERO,
        };
        s.resize(screen_len, entries_len);
        s
    }

    fn resize(&mut self, new_window_len: usize, new_entries_len: usize) {
        self.window_len = new_window_len;
        self.entries_len = new_entries_len;

        self.viewport_reset();
        self.first();
    }

    fn viewport_reset(&mut self) {
        self.viewport = Vec2(0, min(self.window_len, self.entries_len));
    }

    /// Calculate the range in the entries array that can be seem in the screen
    fn visible(&self) -> ops::Range<usize> {
        if self.entries_len < self.window_len {
            return 0..self.entries_len;
        }

        self.viewport.into()
    }

    fn selected(&self) -> usize {
        self.selected_entry
    }

    fn down(&mut self) {
        if self.selected_entry < self.entries_len - 1 && self.entries_len > 0 {
            self.selected_entry += 1;
            if self.entry_overflow(
                sat_inc(self.selected_entry, self.entries_len - 1), // We decrement one because we show "..." when there is overflow
            ) {
                // If overflow just move the viewport up, keep the selection.
                // By moving the entries and not the selection we move the selected entry on screen.
                self.viewport = self.viewport + vec2::ONE;
            } else {
                self.selected_line += 1;
            }
        }
    }

    fn up(&mut self) {
        if self.selected_entry > 0 && self.entries_len > 0 {
            self.selected_entry -= 1;
            // If overflow just move the viewport up, keep the selection.
            // By moving the entries and not the selection we move the selected entry on screen.
            if self.entry_underflow(
                sat_dec(self.selected_entry, 0), // We increment one because we show "..." when there is overflow
            ) {
                self.viewport = self.viewport - vec2::ONE;
            } else {
                self.selected_line = self.selected_line.saturating_sub(1);
            }
        }
    }

    fn first(&mut self) {
        self.selected_entry = 0;
        self.selected_line = 0;
        self.viewport_reset();
    }

    fn last(&mut self) {
        let last_entry_idx = self.entries_len - 1;
        self.selected_line = last_entry_idx;
        self.selected_entry = last_entry_idx;

        self.viewport = Vec2(
            sat_sub(self.entries_len, self.window_len, 0),
            self.entries_len,
        );
    }

    fn scroll_down(&mut self) {
        todo!("implement")
    }

    fn scroll_up(&mut self) {
        todo!("implement")
    }

    /// Checks if the entry at index `i` can be drawn on the window
    fn entry_overflow(&self, i: usize) -> bool {
        i > 0 // Is there anything?
            && self.entries_len > self.window_len // Does the entries not fit the window?
            && i >= self.viewport.1 // Is it outside viewport?
    }

    fn entry_underflow(&self, i: usize) -> bool {
        self.entries_len > self.window_len // Does the entries not fit the window?
            && i < self.viewport.0 // Is it outside viewport?
    }
}

enum StateMsg {
    Ok,
    Info(String),
    Error(String),
}

// TODO: Maybe store some info inside the mode?
#[derive(PartialEq, Eq)]
enum Mode {
    Explorer,
    Command,
}

struct Dune {
    pub vterm: Arc<Mutex<VTerm>>,
    should_quit: bool,

    entries: Vec<file_info::FileInfo>,
    entries_scrolling_window: ScrollingWindow,

    curr_dir: file_info::FileInfo,
    delta_time: time::Duration,
    state: StateMsg,
    mode: Mode,
    prompt: String,
    cursor: (usize, usize),
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
    fn new(
        vterm: Arc<Mutex<VTerm>>,
        key_bindings: KeyBindings,
        starting_path: path::PathBuf,
    ) -> Self {
        Self {
            vterm: vterm.clone(),
            should_quit: false,
            entries: Vec::new(),
            curr_dir: starting_path
                .try_into()
                .expect("could not open current directory"),
            delta_time: time::Duration::ZERO,
            state: StateMsg::Ok,
            mode: Mode::Explorer,
            entries_scrolling_window: ScrollingWindow::new(0, 0), // Hack cus we can't reference self.entries here yet.
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
    pub fn run(&mut self) -> io::Result<&path::Path> {
        VTerm::clear()?;
        self.update_entries()?;
        self.update_panels_size();
        self.render()?;

        loop {
            let start = time::Instant::now();

            if self.should_quit {
                return Ok(self.curr_dir.path());
            }

            self.poll_events()?;

            self.render()?;

            self.delta_time = time::Instant::now() - start;
        }
    }

    fn render(&mut self) -> io::Result<()> {
        let term_size = self.vterm.lock().unwrap().size();
        if term_size.0 <= 22 || term_size.1 <= 9 {
            execute!(
                io::stdout(),
                cursor::MoveTo(0, 0),
                style::PrintStyledContent(
                    style::ContentStyle::new()
                        .bold()
                        .red()
                        .reverse()
                        .apply(" NO SPACE TO DRAW ")
                ),
                cursor::MoveTo(0, 1),
                style::PrintStyledContent(
                    style::ContentStyle::new()
                        .bold()
                        .apply("Please resize window")
                ),
                cursor::MoveTo(0, 2),
                style::PrintStyledContent(
                    style::ContentStyle::new().apply("Minimum window size: 22x9")
                ),
            )?;

            return Ok(());
        }

        match self.mode {
            Mode::Explorer => {
                VTerm::cursor_hide()?;
            }

            Mode::Command => {
                self.panel_prompt
                    .draw_text(&self.prompt, 0, 0, style::ContentStyle::new());
                VTerm::cursor_show()?;
            }
        }

        // Draw state

        let (text, style) = if DEBUG_MODE {
            // Draw debug on state
            let text = format!(
                "view_window: {view_window:?}",
                view_window = self.entries_scrolling_window,
            );
            let style = style::ContentStyle::new().on_white().black().bold();
            (text, style)
        } else {
            match &self.state {
                StateMsg::Error(msg) => (
                    format!("ERROR: {msg}."),
                    style::ContentStyle::new().on_dark_red().white().bold(),
                ),
                StateMsg::Ok => ("".to_owned(), style::ContentStyle::new().on_white().black()),
                StateMsg::Info(msg) => (
                    msg.to_owned(),
                    style::ContentStyle::new().on_white().black().bold(),
                ),
            }
        };
        self.panel_state.fill(' ', style);
        self.panel_state.draw_text(&text, 0, 0, style);

        // Draw header
        let style = style::ContentStyle::new().on_grey();
        self.panel_header.fill(' ', style);
        if self.delta_time == time::Duration::ZERO {
            self.delta_time = time::Duration::from_millis(16);
        }
        let mode = match self.mode {
            Mode::Command => "Command Mode",
            Mode::Explorer => "Explorer Mode",
        };
        let text = format!(
            "{path}: (total {total})",
            path = self.curr_dir.path_str(),
            total = self.entries.len()
        );
        self.panel_header
            .draw_text(&text, 0, 0, style.bold().black());
        let w = self.vterm.lock().unwrap().width;
        self.panel_header
            .draw_text(mode, w - 1 - mode.len(), 0, style.bold().black());

        // Draw entries
        let visible_entries_range = self.entries_scrolling_window.visible();
        for (line_idx, entry_idx) in visible_entries_range.clone().enumerate() {
            if line_idx == 0 && entry_idx > 0 {
                self.panel_file_name
                    .draw_text("...", 3, line_idx, style::ContentStyle::new());
                continue;
            }

            if line_idx == self.panel_file_name.height - 1
                && self.entries.len() > visible_entries_range.end
            {
                self.panel_file_name
                    .draw_text("...", 3, line_idx, style::ContentStyle::new());
                continue;
            }

            self.render_entry(entry_idx, line_idx);
        }

        self.render_terminal()?;

        // Cursor
        self.vterm
            .lock()
            .unwrap()
            .cursor_move(self.cursor.0, self.cursor.1)?;

        Ok(())
    }

    fn render_entry(&mut self, entry_idx: usize, line_idx: usize) {
        let entry = &self.entries[entry_idx];

        let style = if entry_idx == self.entries_scrolling_window.selected() {
            match self.mode {
                Mode::Command => style::ContentStyle::new().bold().on_dark_green(),
                Mode::Explorer => style::ContentStyle::new().bold().reverse(),
            }
        } else {
            style::ContentStyle::new().bold()
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

        let style = if entry.name().starts_with('.') {
            // Unix hidden
            style.dim()
        } else {
            style
        };

        let mut name = entry.name().to_string();
        if name.len() > self.panel_file_name.width {
            // TODO: Maybe do this with `format!`?
            name.truncate(self.panel_file_name.width.saturating_sub(3));
            name.push_str("...");
        }
        self.panel_file_name.draw_text(&name, 0, line_idx, style);

        self.panel_file_last_modified.draw_text(
            entry
                .last_modified()
                .format("%e %b %y")
                .to_string()
                .as_str(),
            0,
            line_idx,
            style::ContentStyle::new().dim(),
        );

        self.panel_file_size.draw_text(
            &entry.pretty_size(),
            0,
            line_idx,
            style::ContentStyle::new().dim(),
        );

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
            line_idx,
            style::ContentStyle::new().dim(),
        );
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
            const PERMISSIONS_LEN: usize = 12;
            const SIZE_LEN: usize = 8;
            const LAST_MODIFIED_LEN: usize = 10;
            let mut len_left = w; // Lenght of the fixed elements on the table

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

        self.entries_scrolling_window
            .resize(self.panel_file_name.height, self.entries.len());
    }

    fn update_entries(&mut self) -> io::Result<()> {
        // Other entries
        let curr_dir = env::current_dir()?;

        self.entries.clear();
        for entry in fs::read_dir(&curr_dir)? {
            self.entries.push(entry?.try_into()?);
        }
        self.entries_scrolling_window
            .resize(self.panel_file_name.height, self.entries.len());

        self.curr_dir = curr_dir.try_into()?;

        Ok(())
    }

    fn render_terminal(&mut self) -> io::Result<()> {
        self.vterm.lock().unwrap().flush()?;
        self.vterm.lock().unwrap().cursor_move(0, 1)
    }

    fn poll_events(&mut self) -> io::Result<()> {
        self.handle_event(event::read()?)
        // TODO: Wait for a few millis to se if any event comes right after the first one.
    }

    fn handle_event(&mut self, evt: event::Event) -> io::Result<()> {
        // Special events
        if let event::Event::Resize(w, h) = evt {
            self.vterm.lock().unwrap().width = w as usize;
            self.vterm.lock().unwrap().height = h as usize;
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
                                let mut exec = process::Command::new(cmd);
                                // TODO: Allow patterns in args for selected file
                                let exec = exec.args(args);
                                // TODO: How are we dealing with user interaction?
                                // TODO: Don't quit on error (if command doesn't exist it will error).
                                let output = exec.output()?;
                                // TODO: Extract signal from ext code.
                                let exit_code = output.status.code().unwrap_or(0);
                                let pretty_command = format!(
                                    "{program} {args}",
                                    program = exec
                                        .get_program()
                                        .to_str()
                                        .unwrap_or("<INVALID-UTF8-PROGRAM>"),
                                    args = exec
                                        .get_args()
                                        .map(|arg| arg.to_str().unwrap_or("<INVALID-UTF8-ARG>"))
                                        .collect::<Vec<_>>()
                                        .join(" ")
                                );
                                if output.status.success() {
                                    let stdout = str::from_utf8(&output.stdout).map_err(|e| {
                                        io::Error::new(io::ErrorKind::InvalidData, e)
                                    })?;
                                    self.state = StateMsg::Info(format!(
                                        "{pretty_command}: exit {exit_code}: {stdout}"
                                    ));
                                } else {
                                    let stderr = str::from_utf8(&output.stderr).map_err(|e| {
                                        io::Error::new(io::ErrorKind::InvalidData, e)
                                    })?;
                                    self.state = StateMsg::Error(format!(
                                        "{pretty_command}: exit {exit_code}: {stderr}"
                                    ));
                                }
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
                        event::Event::Key(event::KeyEvent {
                            code: event::KeyCode::Char(ch),
                            kind: event::KeyEventKind::Press,
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
                        ActionExplorer::NavLineUp => {
                            self.entries_scrolling_window.up();
                        }

                        ActionExplorer::NavLineDown => {
                            self.entries_scrolling_window.down();
                        }

                        ActionExplorer::NavHome => {
                            self.entries_scrolling_window.first();
                        }

                        ActionExplorer::NavEnd => {
                            self.entries_scrolling_window.last();
                        }

                        ActionExplorer::DirEnter => {
                            if let Some(entry) =
                                self.entries.get(self.entries_scrolling_window.selected())
                            {
                                if !entry.is_dir() {
                                    match open::that(entry.path()) {
                                        Ok(()) => self.state = StateMsg::Ok,
                                        Err(e) => {
                                            self.state = StateMsg::Error(format!(
                                                "Tried to open `{f}`, but failed: {err_msg}",
                                                f = entry.name(),
                                                err_msg = e
                                            ))
                                        }
                                    }
                                } else if let Err(err) = cd(entry.name()) {
                                    self.state = StateMsg::Error(format!(
                                        "Tried to enter `{f}`, but failed because {err}",
                                        f = entry.name()
                                    ))
                                } else {
                                    self.update_entries()?;
                                    self.entries_scrolling_window.first();
                                    self.state = StateMsg::Ok;
                                }
                            } else {
                                unreachable!("Selected line is out of bounds");
                            }
                            // TODO: handle errors (file is not dir, no permissions...), print then on status bar?
                        }

                        ActionExplorer::DirLeave => {
                            cd("..")?;
                            self.update_entries()?;
                            self.entries_scrolling_window.first();
                            self.state = StateMsg::Ok;
                        }

                        ActionExplorer::EntriesUpdate => self.update_entries()?,
                    }
                }
            }
        }

        Ok(())
    }

    fn unknown_event(&mut self, _evt: event::Event) {
        // For now, don't do anything...
    }
}

fn cd<P: AsRef<path::Path>>(dir: P) -> io::Result<()> {
    env::set_current_dir(dir)
}

fn main() -> process::ExitCode {
    let starting_dir = env::current_dir().unwrap_or_else(|e| {
        eprintln!("ERROR: {e:?}");
        ".".into() // Default to `.` as last choice
    });

    let mut app = Dune::new(
        Arc::new(Mutex::new(VTerm::new())),
        key_bindings::new(),
        starting_dir,
    );

    let path = match app.run() {
        Err(e) => {
            eprintln!("ERROR: {e:?}");
            return process::ExitCode::FAILURE;
        }
        Ok(path) => path,
    };

    // Used to cd to a dir after quitting.
    // The user will have an alias, that after executing dune, will cd to the contents of the `/tmp/dune-cd.txt` file.
    // This solution is not great. But it's good enough for now.
    // TODO: Is there a better solution?
    if let Err(e) = fs::write("/tmp/dune-cd.txt", path.to_str().unwrap_or(".")) {
        eprintln!("ERROR: {e:?}");
        return process::ExitCode::FAILURE;
    }

    process::ExitCode::SUCCESS
}
