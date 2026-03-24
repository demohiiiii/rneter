use super::{CommandExecutionStrategy, DeviceHandler};

const EXIT_STATUS_SUFFIX: &str = ":__";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedCommandOutput {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub output: String,
}

impl DeviceHandler {
    /// Enable shell exit-status based command success parsing for interactive shells.
    #[cfg(test)]
    pub(crate) fn with_shell_exit_status_marker(mut self, marker: impl Into<String>) -> Self {
        self.command_execution = CommandExecutionStrategy::ShellExitStatus {
            marker: marker.into(),
        };
        self
    }

    pub(crate) fn prepare_command_for_execution(
        &self,
        command: &str,
        capture_exit_status: bool,
    ) -> String {
        if !capture_exit_status {
            return command.to_string();
        }

        match &self.command_execution {
            CommandExecutionStrategy::PromptDriven => command.to_string(),
            CommandExecutionStrategy::ShellExitStatus { marker } => {
                format!(
                    r#"{command}; printf '\n{}%s{}\n' "$?""#,
                    marker, EXIT_STATUS_SUFFIX
                )
            }
        }
    }

    pub(crate) fn finalize_command_output(
        &self,
        output: &str,
        fallback_success: bool,
        capture_exit_status: bool,
    ) -> ParsedCommandOutput {
        if !capture_exit_status {
            return ParsedCommandOutput {
                success: fallback_success,
                exit_code: None,
                output: output.to_string(),
            };
        }

        match &self.command_execution {
            CommandExecutionStrategy::PromptDriven => ParsedCommandOutput {
                success: fallback_success,
                exit_code: None,
                output: output.to_string(),
            },
            CommandExecutionStrategy::ShellExitStatus { marker } => {
                if let Some((exit_code, sanitized)) = parse_shell_exit_status(output, marker) {
                    ParsedCommandOutput {
                        success: exit_code == 0,
                        exit_code: Some(exit_code),
                        output: sanitized,
                    }
                } else {
                    ParsedCommandOutput {
                        success: fallback_success,
                        exit_code: None,
                        output: output.to_string(),
                    }
                }
            }
        }
    }
}

fn parse_shell_exit_status(output: &str, marker: &str) -> Option<(i32, String)> {
    let mut exit_code = None;
    let mut sanitized = String::with_capacity(output.len());

    for segment in output.split_inclusive('\n') {
        let trimmed = segment.trim_end_matches(['\r', '\n']);
        if let Some(code_str) = trimmed
            .strip_prefix(marker)
            .and_then(|rest| rest.strip_suffix(EXIT_STATUS_SUFFIX))
            && let Ok(code) = code_str.parse::<i32>()
        {
            exit_code = Some(code);
            continue;
        }
        sanitized.push_str(segment);
    }

    if !output.ends_with('\n') {
        let trailing = output
            .rsplit('\n')
            .next()
            .filter(|line| !line.is_empty() && !sanitized.ends_with(line));
        if let Some(line) = trailing
            && let Some(code_str) = line
                .strip_prefix(marker)
                .and_then(|rest| rest.strip_suffix(EXIT_STATUS_SUFFIX))
        {
            if let Ok(code) = code_str.parse::<i32>() {
                exit_code = Some(code);
            } else {
                sanitized.push_str(line);
            }
        }
    }

    exit_code.map(|code| (code, sanitized))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::build_test_handler;

    #[test]
    fn shell_exit_status_wrapper_appends_marker_printer() {
        let handler = build_test_handler().with_shell_exit_status_marker("__MARK__:");
        let wrapped = handler.prepare_command_for_execution("echo hi", true);
        assert!(wrapped.contains("echo hi; printf"));
        assert!(wrapped.contains("__MARK__:%s:__"));
    }

    #[test]
    fn parse_shell_exit_status_extracts_code_and_removes_marker_line() {
        let parsed = parse_shell_exit_status("echo hi\nhi\n__MARK__:7:__\nuser@host$", "__MARK__:")
            .expect("parse exit status");

        assert_eq!(parsed.0, 7);
        assert_eq!(parsed.1, "echo hi\nhi\nuser@host$");
    }

    #[test]
    fn finalize_command_output_uses_exit_code_over_fallback_success() {
        let handler = build_test_handler().with_shell_exit_status_marker("__MARK__:");
        let parsed =
            handler.finalize_command_output("cmd\nboom\n__MARK__:0:__\nuser@host$", false, true);

        assert!(parsed.success);
        assert_eq!(parsed.exit_code, Some(0));
        assert_eq!(parsed.output, "cmd\nboom\nuser@host$");
    }
}
