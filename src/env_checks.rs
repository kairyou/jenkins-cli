use colored::*;
use std::env;

use crate::t;
use crate::utils;

/// Check if the current terminal is `mintty`
#[allow(dead_code)]
pub fn is_mintty() -> bool {
    if let Ok(term_program) = env::var("TERM_PROGRAM") {
        return term_program == "mintty";
    }
    false
}

/// Check if the current terminal is under winpty
// #[allow(dead_code)]
// pub fn is_under_winpty() -> bool {
//     if let Ok(_wt) = env::var("WT_SESSION") {
//         // WT_SESSION exists indicates running in Windows Terminal
//         return false;
//     }
//     if let Ok(term) = env::var("TERM") {
//         // winpty usually sets TERM=cygwin
//         return term == "cygwin";
//     }
//     false
// }

/// Determine if the current terminal is unsupported and return TERM_PROGRAM information
///  let (unsupported, term_info) = is_terminal_unsupported();
pub fn is_terminal_unsupported() -> (bool, Option<String>) {
    if let Ok(term_program) = env::var("TERM_PROGRAM") {
        if term_program == "mintty" {
            if let Ok(term_version) = env::var("TERM_PROGRAM_VERSION") {
                if utils::version_compare(&term_version, "3.6.4", "<") {
                    return (true, Some(term_program)); // mintty version is too low
                }
            }
        }
        (false, Some(term_program)) // Supported terminal
    } else {
        (false, None) // No TERM_PROGRAM information
    }
}

/// Prompt the user to upgrade Git Bash terminal
pub fn prompt_upgrade_git_bash() {
    println!("{}", t!("git-bash-version-low").red());
    println!("{}", t!("git-win-download-link").cyan());
    println!();
    println!("{}", t!("alternative-solutions").yellow());
    println!("{}", t!("use-other-win-terminals").cyan());
    println!("{}", t!("use-winpty").cyan());
    println!("{}", t!("winpty-example").green());
}

/// Check if the current terminal is unsupported
pub fn check_unsupported_terminal() {
    let (unsupported, term_info) = is_terminal_unsupported();

    if unsupported {
        if let Some(term) = term_info {
            // If the current terminal is mintty and the version is too low, prompt the user to upgrade Git Bash
            if term == "mintty" {
                prompt_upgrade_git_bash()
            } else {
                println!("{}", t!("unsupported-terminal").red());
            }
        } else {
            println!("{}", t!("unsupported-terminal").red());
        }
        std::process::exit(1);
    }
}
