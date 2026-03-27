use crate::error::ConnectError;
use crate::session::{Command, CommandFlow, CommandInteraction, PromptResponseRule};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;

fn invalid_template(message: impl Into<String>) -> ConnectError {
    ConnectError::InvalidCommandFlowTemplate(message.into())
}

fn default_true() -> bool {
    true
}

fn default_var_kind() -> CommandFlowTemplateVarKind {
    CommandFlowTemplateVarKind::String
}

/// Structured text expression used by command-flow templates.
///
/// This keeps the same overall shape as the TOML design (`vars`, `steps`,
/// `prompts`, conditional branches), but stays fully native to Rust instead of
/// introducing a separate parser or rendering engine.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommandFlowTemplateText {
    Literal {
        value: String,
    },
    Var {
        name: String,
    },
    Concat {
        parts: Vec<CommandFlowTemplateText>,
    },
    IfEquals {
        var: String,
        value: String,
        then_text: Box<CommandFlowTemplateText>,
        #[serde(default)]
        else_text: Option<Box<CommandFlowTemplateText>>,
    },
}

impl CommandFlowTemplateText {
    /// Build a literal text node.
    pub fn literal(value: impl Into<String>) -> Self {
        Self::Literal {
            value: value.into(),
        }
    }

    /// Render the value of one runtime variable as text.
    pub fn var(name: impl Into<String>) -> Self {
        Self::Var { name: name.into() }
    }

    /// Concatenate multiple text nodes.
    pub fn concat(parts: Vec<Self>) -> Self {
        Self::Concat { parts }
    }

    /// Render `then_text` when `var == value`, otherwise `else_text`.
    pub fn if_equals(
        var: impl Into<String>,
        value: impl Into<String>,
        then_text: Self,
        else_text: Option<Self>,
    ) -> Self {
        Self::IfEquals {
            var: var.into(),
            value: value.into(),
            then_text: Box::new(then_text),
            else_text: else_text.map(Box::new),
        }
    }

    fn render(&self, values: &Map<String, Value>) -> String {
        match self {
            Self::Literal { value } => value.clone(),
            Self::Var { name } => values
                .get(name)
                .map(render_value_as_text)
                .unwrap_or_default(),
            Self::Concat { parts } => {
                let mut rendered = String::new();
                for part in parts {
                    rendered.push_str(&part.render(values));
                }
                rendered
            }
            Self::IfEquals {
                var,
                value,
                then_text,
                else_text,
            } => {
                let matches = values
                    .get(var)
                    .map(render_value_as_text)
                    .is_some_and(|current| current == *value);

                if matches {
                    then_text.render(values)
                } else if let Some(otherwise) = else_text {
                    otherwise.render(values)
                } else {
                    String::new()
                }
            }
        }
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

/// Declarative reusable definition for an interactive command flow.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct CommandFlowTemplate {
    /// Stable template identifier.
    pub name: String,
    /// Optional human-readable summary of the workflow.
    #[serde(default)]
    pub description: Option<String>,
    /// Variables consumed by step and prompt templates.
    #[serde(default)]
    pub vars: Vec<CommandFlowTemplateVar>,
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
            description: None,
            vars: Vec::new(),
            stop_on_error: true,
            default_mode: None,
            steps,
        }
    }

    /// Attach a human-readable description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Replace the variable metadata list.
    pub fn with_vars(mut self, vars: Vec<CommandFlowTemplateVar>) -> Self {
        self.vars = vars;
        self
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
        let resolved_vars = self.resolve_runtime_vars(&runtime.vars)?;
        let context = build_command_flow_values(self, runtime, resolved_vars);
        let fallback_mode = runtime
            .default_mode
            .as_deref()
            .or(self.default_mode.as_deref())
            .unwrap_or_default()
            .to_string();

        let mut steps = Vec::with_capacity(self.steps.len());
        for step in &self.steps {
            let command = step.command.render(&context);
            if command.trim().is_empty() {
                return Err(invalid_template(format!(
                    "template '{}' rendered an empty command",
                    self.name
                )));
            }

            let mode = if let Some(mode_template) = &step.mode {
                let rendered = mode_template.render(&context);
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

                let mut response = prompt.response.render(&context);
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

        let mut seen = HashSet::new();
        for field in &self.vars {
            let name = field.name.trim();
            if name.is_empty() {
                return Err(invalid_template(format!(
                    "template '{}' contains a var with an empty name",
                    self.name
                )));
            }
            if !is_safe_var_name(name) {
                return Err(invalid_template(format!(
                    "template '{}' has invalid var name '{}'",
                    self.name, field.name
                )));
            }
            if !seen.insert(name.to_string()) {
                return Err(invalid_template(format!(
                    "template '{}' contains duplicate var '{}'",
                    self.name, field.name
                )));
            }
            if let Some(default_value) = &field.default_value {
                field.validate_value(default_value)?;
            }
        }

        Ok(())
    }

    fn resolve_runtime_vars(&self, raw_vars: &Value) -> Result<Map<String, Value>, ConnectError> {
        let mut vars = match raw_vars {
            Value::Null => Map::new(),
            Value::Object(map) => map.clone(),
            _ => {
                return Err(invalid_template(format!(
                    "template '{}' expects vars to be a JSON object",
                    self.name
                )));
            }
        };

        for field in &self.vars {
            let key = field.name.trim();
            let treat_as_missing =
                !vars.contains_key(key) || vars.get(key).is_some_and(Value::is_null);

            if treat_as_missing {
                vars.remove(key);
                if let Some(default_value) = &field.default_value {
                    vars.insert(key.to_string(), default_value.clone());
                    continue;
                }
                if field.required {
                    return Err(invalid_template(format!(
                        "template '{}' is missing required var '{}'",
                        self.name, field.name
                    )));
                }
                continue;
            }

            if let Some(value) = vars.get(key) {
                field.validate_value(value)?;
            }
        }

        Ok(vars)
    }
}

/// One step inside a reusable command-flow template.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct CommandFlowTemplateStep {
    /// Structured command renderer.
    pub command: CommandFlowTemplateText,
    /// Optional structured mode override.
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
    pub fn new(command: CommandFlowTemplateText) -> Self {
        Self {
            command,
            mode: None,
            timeout_secs: None,
            prompts: Vec::new(),
        }
    }

    /// Override the step mode renderer.
    pub fn with_mode(mut self, mode: CommandFlowTemplateText) -> Self {
        self.mode = Some(mode);
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
    /// Structured response renderer.
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
    pub fn new(patterns: Vec<String>, response: CommandFlowTemplateText) -> Self {
        Self {
            patterns,
            response,
            append_newline: false,
            record_input: false,
        }
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

/// Supported variable kinds for structured command-flow templates.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandFlowTemplateVarKind {
    String,
    Secret,
    Number,
    Boolean,
    Json,
}

impl CommandFlowTemplateVarKind {
    fn validate_value(self, value: &Value) -> bool {
        match self {
            Self::String | Self::Secret => value.is_string(),
            Self::Number => value.is_number(),
            Self::Boolean => value.is_boolean(),
            Self::Json => true,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Secret => "secret",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::Json => "json",
        }
    }
}

/// Variable metadata exposed by a reusable command-flow template.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct CommandFlowTemplateVar {
    /// Variable name referenced by the template.
    pub name: String,
    /// Optional display label for UI/forms.
    #[serde(default)]
    pub label: Option<String>,
    /// Optional human-readable description.
    #[serde(default)]
    pub description: Option<String>,
    /// Value type expected at runtime.
    #[serde(rename = "type", default = "default_var_kind")]
    pub kind: CommandFlowTemplateVarKind,
    /// Whether callers must provide a value when no default exists.
    #[serde(default)]
    pub required: bool,
    /// Optional placeholder value for UI/forms.
    #[serde(default)]
    pub placeholder: Option<String>,
    /// Optional list of allowed string values.
    #[serde(default)]
    pub options: Vec<String>,
    /// Optional default value when the caller does not provide one.
    #[serde(rename = "default", default)]
    pub default_value: Option<Value>,
}

impl CommandFlowTemplateVar {
    /// Build variable metadata for one named runtime value.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            label: None,
            description: None,
            kind: default_var_kind(),
            required: false,
            placeholder: None,
            options: Vec::new(),
            default_value: None,
        }
    }

    /// Attach a human-readable label.
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Attach a human-readable description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Override the expected value type.
    pub fn with_kind(mut self, kind: CommandFlowTemplateVarKind) -> Self {
        self.kind = kind;
        self
    }

    /// Mark the variable as required.
    pub fn with_required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    /// Attach a placeholder value for UI/forms.
    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// Restrict the variable to one of the provided string options.
    pub fn with_options<I, S>(mut self, options: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.options = options.into_iter().map(Into::into).collect();
        self
    }

    /// Set a default runtime value.
    pub fn with_default_value(mut self, default_value: Value) -> Self {
        self.default_value = Some(default_value);
        self
    }

    /// Human-friendly label, falling back to the variable name.
    pub fn display_label(&self) -> &str {
        self.label
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(self.name.as_str())
    }

    fn validate_value(&self, value: &Value) -> Result<(), ConnectError> {
        if !self.kind.validate_value(value) {
            return Err(invalid_template(format!(
                "var '{}' expected {}",
                self.name,
                self.kind.label()
            )));
        }

        if !self.options.is_empty() && !matches!(self.kind, CommandFlowTemplateVarKind::Json) {
            let Some(text) = value.as_str() else {
                return Err(invalid_template(format!(
                    "var '{}' expected one of [{}]",
                    self.name,
                    self.options.join(", ")
                )));
            };
            if !self.options.iter().any(|option| option == text) {
                return Err(invalid_template(format!(
                    "var '{}' expected one of [{}]",
                    self.name,
                    self.options.join(", ")
                )));
            }
        }

        Ok(())
    }
}

/// Runtime values used to render a structured command-flow template.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct CommandFlowTemplateRuntime {
    /// Per-render default mode. Falls back to template `default_mode`.
    #[serde(default)]
    pub default_mode: Option<String>,
    /// Optional connection name exposed to the renderer.
    #[serde(default)]
    pub connection_name: Option<String>,
    /// Optional host exposed to the renderer.
    #[serde(default)]
    pub host: Option<String>,
    /// Optional username exposed to the renderer.
    #[serde(default)]
    pub username: Option<String>,
    /// Optional device profile exposed to the renderer.
    #[serde(default)]
    pub device_profile: Option<String>,
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
    vars.insert(
        "default_mode".to_string(),
        runtime
            .default_mode
            .clone()
            .or_else(|| template.default_mode.clone())
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    vars.insert(
        "connection_name".to_string(),
        runtime
            .connection_name
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    vars.insert(
        "host".to_string(),
        runtime
            .host
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    vars.insert(
        "username".to_string(),
        runtime
            .username
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    vars.insert(
        "device_profile".to_string(),
        runtime
            .device_profile
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    vars
}

fn is_safe_var_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(ch) if ch.is_ascii_alphabetic() || ch == '_' => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn renders_template_with_conditional_text() {
        let template = CommandFlowTemplate::new(
            "demo",
            vec![
                CommandFlowTemplateStep::new(CommandFlowTemplateText::if_equals(
                    "direction",
                    "to_device",
                    CommandFlowTemplateText::concat(vec![
                        CommandFlowTemplateText::literal("copy "),
                        CommandFlowTemplateText::var("protocol"),
                        CommandFlowTemplateText::literal(": "),
                        CommandFlowTemplateText::var("device_path"),
                    ]),
                    Some(CommandFlowTemplateText::concat(vec![
                        CommandFlowTemplateText::literal("copy "),
                        CommandFlowTemplateText::var("device_path"),
                        CommandFlowTemplateText::literal(" "),
                        CommandFlowTemplateText::var("protocol"),
                        CommandFlowTemplateText::literal(":"),
                    ])),
                ))
                .with_timeout_secs(300)
                .with_prompts(vec![
                    CommandFlowTemplatePrompt::new(
                        vec!["(?i)^Address.*$".to_string()],
                        CommandFlowTemplateText::var("server_addr"),
                    )
                    .with_append_newline(true)
                    .with_record_input(true),
                ]),
            ],
        )
        .with_default_mode("Enable")
        .with_vars(vec![
            CommandFlowTemplateVar::new("protocol")
                .with_required(true)
                .with_options(["scp", "tftp"]),
            CommandFlowTemplateVar::new("direction")
                .with_required(true)
                .with_options(["to_device", "from_device"]),
            CommandFlowTemplateVar::new("device_path").with_required(true),
            CommandFlowTemplateVar::new("server_addr").with_required(true),
        ]);

        let flow = template
            .to_command_flow(&CommandFlowTemplateRuntime::new().with_vars(json!({
                "protocol": "scp",
                "direction": "to_device",
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
        let template = CommandFlowTemplate::new(
            "demo",
            vec![CommandFlowTemplateStep::new(CommandFlowTemplateText::var(
                "host",
            ))],
        )
        .with_vars(vec![
            CommandFlowTemplateVar::new("host").with_required(true),
        ]);

        let err = template
            .to_command_flow(&CommandFlowTemplateRuntime::new())
            .expect_err("missing required var should fail");

        assert!(matches!(err, ConnectError::InvalidCommandFlowTemplate(_)));
    }

    #[test]
    fn runtime_vars_must_be_json_object() {
        let template = CommandFlowTemplate::new(
            "demo",
            vec![CommandFlowTemplateStep::new(
                CommandFlowTemplateText::literal("show version"),
            )],
        );

        let err = template
            .to_command_flow(&CommandFlowTemplateRuntime::new().with_vars(json!(["bad"])))
            .expect_err("non-object vars should fail");

        assert!(matches!(err, ConnectError::InvalidCommandFlowTemplate(_)));
    }
}
