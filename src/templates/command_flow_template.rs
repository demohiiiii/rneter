use crate::error::ConnectError;
use crate::session::{Command, CommandFlow, CommandInteraction, PromptResponseRule};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

fn invalid_template(message: impl Into<String>) -> ConnectError {
    ConnectError::InvalidCommandFlowTemplate(message.into())
}

fn default_true() -> bool {
    true
}

/// Lightweight `{{var}}` inline text template used by command-flow templates.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(transparent)]
pub struct CommandFlowTemplateText {
    value: String,
}

impl CommandFlowTemplateText {
    /// Build a lightweight `{{var}}` inline template.
    pub fn template(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
        }
    }

    fn render(&self, values: &Map<String, Value>) -> Result<String, ConnectError> {
        render_inline_template(self.value.as_str(), values)
    }
}

impl From<String> for CommandFlowTemplateText {
    fn from(value: String) -> Self {
        Self::template(value)
    }
}

impl From<&str> for CommandFlowTemplateText {
    fn from(value: &str) -> Self {
        Self::template(value)
    }
}

fn render_value_as_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        other => other.to_string(),
    }
}

fn render_inline_template(
    template: &str,
    values: &Map<String, Value>,
) -> Result<String, ConnectError> {
    let mut output = String::new();
    let mut rest = template;

    while let Some(start) = rest.find("{{") {
        output.push_str(&rest[..start]);
        let after_start = &rest[start + 2..];

        if let Some(end) = after_start.find("}}") {
            let raw_name = &after_start[..end];
            let name = raw_name.trim();
            if name.is_empty() {
                output.push_str("{{");
                output.push_str(raw_name);
                output.push_str("}}");
            } else if let Some(value) = values.get(name).filter(|value| !value.is_null()) {
                output.push_str(&render_value_as_text(value));
            } else {
                return Err(invalid_template(format!("missing template var '{name}'")));
            }
            rest = &after_start[end + 2..];
        } else {
            output.push_str(&rest[start..]);
            rest = "";
            break;
        }
    }

    output.push_str(rest);
    Ok(output)
}

/// Declarative reusable definition for an interactive command flow.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct CommandFlowTemplate {
    /// Stable template identifier.
    pub name: String,
    /// Stop after the first failing step when true.
    #[serde(default = "default_true")]
    pub stop_on_error: bool,
    /// Fallback mode applied when a step omits `mode`.
    #[serde(default)]
    pub default_mode: Option<String>,
    /// Ordered command steps executed on one live session.
    #[serde(default)]
    pub steps: Vec<CommandFlowTemplateStep>,
}

impl CommandFlowTemplate {
    /// Build a template from a name and an ordered list of steps.
    pub fn new(name: impl Into<String>, steps: Vec<CommandFlowTemplateStep>) -> Self {
        Self {
            name: name.into(),
            stop_on_error: true,
            default_mode: None,
            steps,
        }
    }

    /// Override the default mode applied to steps without `mode`.
    pub fn with_default_mode(mut self, default_mode: impl Into<String>) -> Self {
        self.default_mode = Some(default_mode.into());
        self
    }

    /// Control whether rendering should stop after the first failing step.
    pub fn with_stop_on_error(mut self, stop_on_error: bool) -> Self {
        self.stop_on_error = stop_on_error;
        self
    }

    /// Render a command-flow template into a runtime [`CommandFlow`].
    pub fn to_command_flow(
        &self,
        runtime: &CommandFlowTemplateRuntime,
    ) -> Result<CommandFlow, ConnectError> {
        self.validate_definition()?;
        let vars = match &runtime.vars {
            Value::Null => Map::new(),
            Value::Object(map) => map.clone(),
            _ => {
                return Err(invalid_template(format!(
                    "template '{}' expects vars to be a JSON object",
                    self.name
                )));
            }
        };
        let context = build_command_flow_values(self, runtime, vars);
        let fallback_mode = runtime
            .default_mode
            .as_deref()
            .or(self.default_mode.as_deref())
            .unwrap_or_default()
            .to_string();

        let mut steps = Vec::with_capacity(self.steps.len());
        for step in &self.steps {
            let command = step.command.render(&context)?;
            if command.trim().is_empty() {
                return Err(invalid_template(format!(
                    "template '{}' rendered an empty command",
                    self.name
                )));
            }

            let mode = if let Some(mode_template) = &step.mode {
                let rendered = mode_template.render(&context)?;
                let normalized = rendered.trim();
                if normalized.is_empty() {
                    fallback_mode.clone()
                } else {
                    normalized.to_string()
                }
            } else {
                fallback_mode.clone()
            };

            let mut prompts = Vec::with_capacity(step.prompts.len());
            for prompt in &step.prompts {
                if prompt.patterns.is_empty() {
                    return Err(invalid_template(format!(
                        "template '{}' contains a prompt with no patterns",
                        self.name
                    )));
                }

                let mut response = prompt.response.render(&context)?;
                if prompt.append_newline {
                    response.push('\n');
                }
                prompts.push(
                    PromptResponseRule::new(prompt.patterns.clone(), response)
                        .with_record_input(prompt.record_input),
                );
            }

            steps.push(Command {
                mode,
                command,
                timeout: step.timeout_secs,
                dyn_params: Default::default(),
                interaction: CommandInteraction { prompts },
            });
        }

        Ok(CommandFlow {
            steps,
            stop_on_error: self.stop_on_error,
            max_steps: None,
        })
    }

    fn validate_definition(&self) -> Result<(), ConnectError> {
        if self.name.trim().is_empty() {
            return Err(invalid_template("template name cannot be empty"));
        }
        if self.steps.is_empty() {
            return Err(invalid_template(format!(
                "template '{}' has no steps",
                self.name
            )));
        }

        Ok(())
    }
}

/// One step inside a reusable command-flow template.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct CommandFlowTemplateStep {
    /// Inline command template renderer.
    pub command: CommandFlowTemplateText,
    /// Optional mode template override.
    #[serde(default)]
    pub mode: Option<CommandFlowTemplateText>,
    /// Step timeout in seconds.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// Interactive prompt-response rules evaluated while this step runs.
    #[serde(default)]
    pub prompts: Vec<CommandFlowTemplatePrompt>,
}

impl CommandFlowTemplateStep {
    /// Build a step from its command renderer.
    pub fn new(command: impl Into<CommandFlowTemplateText>) -> Self {
        Self {
            command: command.into(),
            mode: None,
            timeout_secs: None,
            prompts: Vec::new(),
        }
    }

    /// Build a step from a lightweight `{{var}}` inline command template.
    pub fn from_template(command: impl Into<String>) -> Self {
        Self::new(CommandFlowTemplateText::template(command))
    }

    /// Override the step mode renderer.
    pub fn with_mode(mut self, mode: impl Into<CommandFlowTemplateText>) -> Self {
        self.mode = Some(mode.into());
        self
    }

    /// Override the step timeout in seconds.
    pub fn with_timeout_secs(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = Some(timeout_secs);
        self
    }

    /// Replace the step prompt list.
    pub fn with_prompts(mut self, prompts: Vec<CommandFlowTemplatePrompt>) -> Self {
        self.prompts = prompts;
        self
    }
}

/// One prompt-response rule inside a reusable command-flow template.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct CommandFlowTemplatePrompt {
    /// Regex patterns that identify the prompt.
    pub patterns: Vec<String>,
    /// Inline response template renderer.
    pub response: CommandFlowTemplateText,
    /// Append `\n` after the rendered response.
    #[serde(default)]
    pub append_newline: bool,
    /// Keep the matched prompt in captured output.
    #[serde(default)]
    pub record_input: bool,
}

impl CommandFlowTemplatePrompt {
    /// Build a prompt-response rule from regex patterns and a response template.
    pub fn new(patterns: Vec<String>, response: impl Into<CommandFlowTemplateText>) -> Self {
        Self {
            patterns,
            response: response.into(),
            append_newline: false,
            record_input: false,
        }
    }

    /// Build a prompt-response rule from a lightweight `{{var}}` inline response template.
    pub fn from_template(patterns: Vec<String>, response: impl Into<String>) -> Self {
        Self::new(patterns, CommandFlowTemplateText::template(response))
    }

    /// Append `\n` after the rendered response.
    pub fn with_append_newline(mut self, append_newline: bool) -> Self {
        self.append_newline = append_newline;
        self
    }

    /// Keep the matched prompt in captured output.
    pub fn with_record_input(mut self, record_input: bool) -> Self {
        self.record_input = record_input;
        self
    }
}

/// Runtime values used to render a structured command-flow template.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct CommandFlowTemplateRuntime {
    /// Per-render default mode. Falls back to template `default_mode`.
    #[serde(default)]
    pub default_mode: Option<String>,
    /// Template vars. Must be a JSON object when provided.
    #[serde(default)]
    pub vars: Value,
}

impl CommandFlowTemplateRuntime {
    /// Build an empty runtime value bag.
    pub fn new() -> Self {
        Self::default()
    }

    /// Override the default mode used when a step omits `mode`.
    pub fn with_default_mode(mut self, default_mode: impl Into<String>) -> Self {
        self.default_mode = Some(default_mode.into());
        self
    }

    /// Replace the template variable bag.
    pub fn with_vars(mut self, vars: Value) -> Self {
        self.vars = vars;
        self
    }
}

fn build_command_flow_values(
    template: &CommandFlowTemplate,
    runtime: &CommandFlowTemplateRuntime,
    mut vars: Map<String, Value>,
) -> Map<String, Value> {
    if let Some(default_mode) = runtime
        .default_mode
        .clone()
        .or_else(|| template.default_mode.clone())
    {
        vars.insert("default_mode".to_string(), Value::String(default_mode));
    }
    vars
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn renders_template_with_inline_text() {
        let template = CommandFlowTemplate::new(
            "demo",
            vec![
                CommandFlowTemplateStep::new("copy {{protocol}}: {{device_path}}")
                    .with_timeout_secs(300)
                    .with_prompts(vec![
                        CommandFlowTemplatePrompt::new(
                            vec!["(?i)^Address.*$".to_string()],
                            "{{server_addr}}",
                        )
                        .with_append_newline(true)
                        .with_record_input(true),
                    ]),
            ],
        )
        .with_default_mode("Enable");

        let flow = template
            .to_command_flow(&CommandFlowTemplateRuntime::new().with_vars(json!({
                "protocol": "scp",
                "device_path": "flash:/image.bin",
                "server_addr": "192.0.2.10",
            })))
            .expect("render flow");

        assert!(flow.stop_on_error);
        assert_eq!(flow.steps.len(), 1);
        assert_eq!(flow.steps[0].mode, "Enable");
        assert_eq!(flow.steps[0].command, "copy scp: flash:/image.bin");
        assert_eq!(
            flow.steps[0].interaction.prompts[0].response,
            "192.0.2.10\n"
        );
    }

    #[test]
    fn missing_required_var_fails_rendering() {
        let template =
            CommandFlowTemplate::new("demo", vec![CommandFlowTemplateStep::new("show {{host}}")]);

        let err = template
            .to_command_flow(&CommandFlowTemplateRuntime::new())
            .expect_err("missing required var should fail");

        assert!(matches!(err, ConnectError::InvalidCommandFlowTemplate(_)));
    }

    #[test]
    fn null_var_fails_but_empty_string_is_explicit() {
        let template =
            CommandFlowTemplate::new("demo", vec![CommandFlowTemplateStep::new("show {{value}}")]);

        let err = template
            .to_command_flow(&CommandFlowTemplateRuntime::new().with_vars(json!({ "value": null })))
            .expect_err("null var should fail");
        assert!(matches!(err, ConnectError::InvalidCommandFlowTemplate(_)));

        let flow = template
            .to_command_flow(&CommandFlowTemplateRuntime::new().with_vars(json!({ "value": "" })))
            .expect("empty string should be explicit");
        assert_eq!(flow.steps[0].command, "show ");
    }

    #[test]
    fn runtime_vars_must_be_json_object() {
        let template =
            CommandFlowTemplate::new("demo", vec![CommandFlowTemplateStep::new("show version")]);

        let err = template
            .to_command_flow(&CommandFlowTemplateRuntime::new().with_vars(json!(["bad"])))
            .expect_err("non-object vars should fail");

        assert!(matches!(err, ConnectError::InvalidCommandFlowTemplate(_)));
    }

    #[test]
    fn inline_template_text_renders_placeholders() {
        let template = CommandFlowTemplate::new(
            "demo",
            vec![CommandFlowTemplateStep::new(
                "copy {{protocol}}: {{device_path}}",
            )],
        );

        let flow = template
            .to_command_flow(&CommandFlowTemplateRuntime::new().with_vars(json!({
                "protocol": "scp",
                "device_path": "flash:/image.bin",
            })))
            .expect("render flow");

        assert_eq!(flow.steps[0].command, "copy scp: flash:/image.bin");
    }

    #[test]
    fn prompt_and_mode_accept_plain_text_builders() {
        let template = CommandFlowTemplate::new(
            "demo",
            vec![
                CommandFlowTemplateStep::new("show {{target}}")
                    .with_mode("{{exec_mode}}")
                    .with_prompts(vec![
                        CommandFlowTemplatePrompt::new(
                            vec!["(?i)^Proceed\\?\\s*$".to_string()],
                            "yes",
                        )
                        .with_append_newline(true),
                    ]),
            ],
        )
        .with_default_mode("Enable");

        let flow = template
            .to_command_flow(&CommandFlowTemplateRuntime::new().with_vars(json!({
                "target": "version",
                "exec_mode": "Config",
            })))
            .expect("render flow");

        assert_eq!(flow.steps[0].mode, "Config");
        assert_eq!(flow.steps[0].command, "show version");
        assert_eq!(flow.steps[0].interaction.prompts[0].response, "yes\n");
    }
}
