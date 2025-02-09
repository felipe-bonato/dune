mod file_info;
mod key_bindings;
mod vterm;

use std::{
    cmp::min,
    env, fs, io, path, process, str,
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
use vterm::{Panel, VTerm};

// TODO: Better separation into modules.
// TODO: Improve text styling, currently it's all over the place
//       it should come from a config file (maybe the same that the keybindings use?).

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
    fn new(
        vterm: Arc<Mutex<VTerm>>,
        key_bindings: KeyBindings,
        starting_path: path::PathBuf,
    ) -> Self {
        Self {
            vterm: vterm.clone(),
            should_quit: false,
            updated_entries: false,
            entries: Vec::new(),
            curr_dir: starting_path
                .try_into()
                .expect("could not open current directory"),
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

        if self.updated_entries {
            self.selected_line = 0;
            self.selected_entry = 0;
            self.updated_entries = false;
        }

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
                    .draw_text("   ...   ", 0, i, style::ContentStyle::new());
                continue;
            }

            if i == 0 && self.view_window.0 > 0 {
                self.panel_file_name
                    .draw_text("   ...   ", 0, i, style::ContentStyle::new());
                continue;
            }

            let style = if i == self.selected_line {
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
            if name.len() > self.panel_file_name.width as usize {
                // TODO: Maybe do this with `format!`?
                name.truncate(self.panel_file_name.width.saturating_sub(3) as usize);
                name.push_str("...");
            }
            self.panel_file_name.draw_text(&name, 0, i, style);

            self.panel_file_last_modified.draw_text(
                entry
                    .last_modified()
                    .format("%e %b %y")
                    .to_string()
                    .as_str(),
                0,
                i,
                style::ContentStyle::new().dim(),
            );

            self.panel_file_size.draw_text(
                &entry.pretty_size(),
                0,
                i,
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
                i,
                style::ContentStyle::new().dim(),
            );
        }

        // Draw state
        let (text, style) = match &self.state {
            StateMsg::Error(msg) => (
                format!("ERROR: {msg}."),
                style::ContentStyle::new().on_dark_red().white().bold(),
            ),
            StateMsg::Ok => ("".to_owned(), style::ContentStyle::new().on_white().black()),
            StateMsg::Info(msg) => (
                msg.to_owned(),
                style::ContentStyle::new().on_white().black().bold(),
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

        Ok(())
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
        self.handle_event(event::read()?)
        // TODO: Wait for a few millis to se if any event comes right after the first one.
    }

    fn handle_event(&mut self, evt: event::Event) -> io::Result<()> {
        // Special events
        if let event::Event::Resize(w, h) = evt {
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
                                let mut exec = process::Command::new(cmd);
                                let exec = exec
                                    .args(args)
                                    .arg(self.entries[self.selected_entry].path_str());
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
