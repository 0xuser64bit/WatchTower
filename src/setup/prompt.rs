//! Interactive and scripted prompting for the setup wizard.

use inquire::{Confirm, InquireError, Password, PasswordDisplayMode, Select, Text};

#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    #[error("setup cancelled")]
    Cancelled,
    #[error("setup requires an interactive terminal")]
    NotInteractive,
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Invalid(String),
}

impl From<InquireError> for SetupError {
    fn from(err: InquireError) -> Self {
        match err {
            InquireError::OperationCanceled | InquireError::OperationInterrupted => {
                SetupError::Cancelled
            }
            other => SetupError::Invalid(other.to_string()),
        }
    }
}

pub trait Prompter {
    fn intro(&mut self, text: &str);
    fn section(&mut self, title: &str, body: &str);
    fn note(&mut self, text: &str);
    fn confirm(&mut self, question: &str, default: bool) -> Result<bool, SetupError>;
    fn text(&mut self, label: &str, default: Option<&str>) -> Result<String, SetupError>;
    fn password(&mut self, label: &str) -> Result<String, SetupError>;
    fn choose(
        &mut self,
        question: &str,
        options: &[&str],
        default: usize,
    ) -> Result<usize, SetupError>;
}

pub struct InquirePrompter;

impl Prompter for InquirePrompter {
    fn intro(&mut self, text: &str) {
        println!();
        println!("{text}");
        println!();
    }

    fn section(&mut self, title: &str, body: &str) {
        println!();
        println!("── {title} {}", "─".repeat(title.len().min(40)));
        println!();
        println!("{body}");
        println!();
    }

    fn note(&mut self, text: &str) {
        println!("{text}");
    }

    fn confirm(&mut self, question: &str, default: bool) -> Result<bool, SetupError> {
        Ok(Confirm::new(question).with_default(default).prompt()?)
    }

    fn text(&mut self, label: &str, default: Option<&str>) -> Result<String, SetupError> {
        let mut prompt = Text::new(label);
        if let Some(default) = default {
            prompt = prompt.with_default(default);
        }
        Ok(prompt.prompt()?)
    }

    fn password(&mut self, label: &str) -> Result<String, SetupError> {
        Ok(Password::new(label)
            .with_display_mode(PasswordDisplayMode::Masked)
            .without_confirmation()
            .prompt()?)
    }

    fn choose(
        &mut self,
        question: &str,
        options: &[&str],
        default: usize,
    ) -> Result<usize, SetupError> {
        let options: Vec<&str> = options.to_vec();
        let starting = default.min(options.len().saturating_sub(1));
        let selected = Select::new(question, options.clone())
            .with_starting_cursor(starting)
            .prompt()?;
        Ok(options
            .iter()
            .position(|option| *option == selected)
            .unwrap_or(starting))
    }
}
