use crate::config::Preset;
use anyhow::{Context, Result, bail};
use std::io::{self, IsTerminal, Write};

pub(crate) fn is_interactive_terminal() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

pub(crate) fn prompt_preset_selection(default: Preset) -> Result<Preset> {
    println!("Choose a preset:");
    for (index, preset) in Preset::choices().iter().enumerate() {
        let suffix = if *preset == default {
            " (recommended)"
        } else {
            ""
        };
        println!("  {}. {}{}", index + 1, preset.as_str(), suffix);
        println!("     {}", preset.description());
    }

    let prompt = format!("Preset [default: {}]", default.as_str());
    let choice = prompt_line(&prompt)?;
    let choice = choice.trim();

    if choice.is_empty() {
        return Ok(default);
    }

    if let Ok(index) = choice.parse::<usize>() {
        if let Some(preset) = Preset::choices().get(index.saturating_sub(1)) {
            return Ok(*preset);
        }
    }

    if let Some(preset) = Preset::from_str(choice) {
        return Ok(preset);
    }

    bail!(
        "unknown preset `{}`. Use one of: agent, audit, minimal.",
        choice
    )
}

pub(crate) fn confirm(prompt: &str) -> Result<bool> {
    print!("{prompt} [y/N] ");
    io::stdout().flush().context("failed to flush stdout")?;

    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .context("failed to read confirmation")?;

    Ok(matches!(line.trim(), "y" | "Y" | "yes" | "YES" | "Yes"))
}

pub(crate) fn confirm_default_yes(prompt: &str) -> Result<bool> {
    print!("{prompt} [Y/n] ");
    io::stdout().flush().context("failed to flush stdout")?;

    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .context("failed to read confirmation")?;

    let trimmed = line.trim();
    Ok(trimmed.is_empty() || matches!(trimmed, "y" | "Y" | "yes" | "YES" | "Yes"))
}

fn prompt_line(prompt: &str) -> Result<String> {
    print!("{prompt}: ");
    io::stdout().flush().context("failed to flush stdout")?;

    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .context("failed to read prompt input")?;

    Ok(line)
}
