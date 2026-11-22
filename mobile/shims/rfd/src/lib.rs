//! Remplaçant de `rfd` pour le téléphone : aucune fenêtre de fichiers, les confirmations sont acceptées.
use std::path::PathBuf;

#[derive(Default)]
pub struct FileDialog;

impl FileDialog {
    pub fn new() -> Self {
        FileDialog
    }
    pub fn add_filter(self, _: impl Into<String>, _: &[impl ToString]) -> Self {
        self
    }
    pub fn set_file_name(self, _: impl Into<String>) -> Self {
        self
    }
    pub fn set_directory(self, _: impl AsRef<std::path::Path>) -> Self {
        self
    }
    pub fn set_title(self, _: impl Into<String>) -> Self {
        self
    }
    pub fn pick_file(self) -> Option<PathBuf> {
        None
    }
    pub fn pick_files(self) -> Option<Vec<PathBuf>> {
        None
    }
    pub fn pick_folder(self) -> Option<PathBuf> {
        None
    }
    pub fn save_file(self) -> Option<PathBuf> {
        None
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MessageButtons {
    #[default]
    Ok,
    OkCancel,
    YesNo,
    YesNoCancel,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MessageLevel {
    #[default]
    Info,
    Warning,
    Error,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum MessageDialogResult {
    Yes,
    No,
    Ok,
    Cancel,
    Custom(String),
}

#[derive(Default)]
pub struct MessageDialog {
    boutons: MessageButtons,
}

impl MessageDialog {
    pub fn new() -> Self {
        MessageDialog::default()
    }
    pub fn set_title(self, _: impl Into<String>) -> Self {
        self
    }
    pub fn set_description(self, _: impl Into<String>) -> Self {
        self
    }
    pub fn set_level(self, _: MessageLevel) -> Self {
        self
    }
    pub fn set_buttons(mut self, b: MessageButtons) -> Self {
        self.boutons = b;
        self
    }
    pub fn show(self) -> MessageDialogResult {
        match self.boutons {
            MessageButtons::Ok | MessageButtons::OkCancel => MessageDialogResult::Ok,
            _ => MessageDialogResult::Yes,
        }
    }
}
