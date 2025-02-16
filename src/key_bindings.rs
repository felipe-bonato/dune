use std::collections::HashMap;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

#[derive(Debug, Copy, Clone)]
pub enum ActionExplorer {
    NavLineUp,
    NavLineDown,
    NavHome,
    NavEnd,
    DirEnter,
    DirLeave,
    EntriesUpdate,
}

#[derive(Debug, Copy, Clone)]
pub enum ActionCommand {
    Execute,
    PromptBackspace,
}

#[derive(Debug, Copy, Clone)]
pub enum ActionGlobal {
    Quit,
    ModeChange,
}

#[derive(Debug, Copy, Clone)]
pub enum Action {
    Explorer(ActionExplorer),
    Command(ActionCommand),
    Global(ActionGlobal),
}

pub struct KeyBindings {
    explorer: HashMap<Event, Action>,
    command: HashMap<Event, Action>,
    global: HashMap<Event, Action>,
}

impl KeyBindings {
    pub fn get_explorer(&mut self, event: &Event) -> Option<&ActionExplorer> {
        if let Some(Action::Explorer(action)) = self.explorer.get(event) {
            Some(action)
        } else {
            None
        }
    }

    pub fn get_command(&mut self, event: &Event) -> Option<&ActionCommand> {
        if let Some(Action::Command(action)) = self.command.get(event) {
            Some(action)
        } else {
            None
        }
    }

    pub fn get_global(&mut self, event: &Event) -> Option<&ActionGlobal> {
        if let Some(Action::Global(action)) = self.global.get(event) {
            Some(action)
        } else {
            None
        }
    }
}

pub fn from_key_code(code: KeyCode) -> Event {
    Event::Key(KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    })
}

pub fn new() -> KeyBindings {
    // TODO: take the key bindings from a file and parse it
    KeyBindings {
        explorer: HashMap::from([
            (
                from_key_code(KeyCode::Up),
                Action::Explorer(ActionExplorer::NavLineUp),
            ),
            (
                from_key_code(KeyCode::Down),
                Action::Explorer(ActionExplorer::NavLineDown),
            ),
            (
                from_key_code(KeyCode::Enter),
                Action::Explorer(ActionExplorer::DirEnter),
            ),
            (
                from_key_code(KeyCode::Backspace),
                Action::Explorer(ActionExplorer::DirLeave),
            ),
            (
                from_key_code(KeyCode::F(5)),
                Action::Explorer(ActionExplorer::EntriesUpdate),
            ),
            (
                from_key_code(KeyCode::Home),
                Action::Explorer(ActionExplorer::NavHome)
            ),
            (
                from_key_code(KeyCode::End),
                Action::Explorer(ActionExplorer::NavEnd)
            )
        ]),
        command: HashMap::from([
            (
                from_key_code(KeyCode::Enter),
                Action::Command(ActionCommand::Execute),
            ),
            (
                from_key_code(KeyCode::Backspace),
                Action::Command(ActionCommand::PromptBackspace),
            ),
        ]),
        global: HashMap::from([
            (
                Event::Key(KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers: KeyModifiers::CONTROL,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                Action::Global(ActionGlobal::Quit),
            ),
            (
                from_key_code(KeyCode::Tab),
                Action::Global(ActionGlobal::ModeChange),
            ),
        ]),
    }
}

// register_cmd!(
//     evt: crossterm::Event,
//     when: impl Fn(ctx: Ctx) -> bool,
//     emit: Cmd
// )
