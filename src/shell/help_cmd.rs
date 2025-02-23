use lazy_static::lazy_static;
use std::collections::{HashMap, HashSet};
use super::{
    types::*
};

// Define the HashSet of commands & command descriptions
lazy_static! {
        pub static ref BUILTIN_COMMANDS: HashSet<&'static str> = {
            vec![
                "exit", "echo", "type", "pwd", "help", "drive-space",
                "file-type-dist", "largest-files", "largest-folder",
                "recent-large-files", "old-large-files", "full-drive-analysis"
            ]
                .into_iter()
                .collect()
        };
        pub static ref COMMAND_DESCRIPTIONS: HashMap<&'static str, CommandInfo> = {
            let mut m = HashMap::new();
            m.insert("help", CommandInfo {
                title: "Help",
                description: "Displays all commands descriptions \n\
                if an argument is given, it gives the command description of the said argument"
            });
            m.insert("exit", CommandInfo {
                title: "Exit",
                description: "hey, you, yes you, if you can read this and understand it, \n\
                then there is no need for an explanation of what this command does"
            });
            m
        };
    }