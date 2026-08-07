use super::super::*;
use crate::device::{latest_terminal_fragment, normalize_terminal_output};
use crate::templates::{DetectSnapshot, summarize_detect_log_text};
use log::{debug, trace};
use regex::Regex;
use std::collections::HashMap;

fn normalize_detect_output(raw: &str) -> String {
    normalize_terminal_output(raw)
}

fn looks_like_shell_prompt(fragment: &str) -> bool {
    static PROMPT_RE: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
        Regex::new(r"(?m)^(?:<[^>\n]+>|\[[^\]\n]+\]|\S+@\S+[>#]|\S.*[#$>%])\s*$")
            .expect("valid detect prompt regex")
    });
    PROMPT_RE.is_match(fragment.trim_end())
}

fn looks_like_pager_prompt(fragment: &str) -> bool {
    static PAGER_RE: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
        Regex::new(r"(?i)^\s*(?:<---\s*More\s*--->|--\s*More\s*--|----\s*More\s*----|More:?)\s*$")
            .expect("valid detect pager regex")
    });
    PAGER_RE.is_match(fragment.trim())
}

fn should_strip_prompt_prefix(line: &str) -> bool {
    static PROMPT_PREFIX_RE: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
        Regex::new(r"^(?:\[[^\]\n]+\]|<[^>\n]+>)\s*$").expect("valid detect prompt prefix regex")
    });
    PROMPT_PREFIX_RE.is_match(line.trim())
}

fn build_probe_timeout_message(
    command: &str,
    expected_prompt: &str,
    last_fragment: &str,
    raw_output: &str,
) -> String {
    format!(
        "autodetect probe timeout: {} waiting_for_prompt='{}' last_fragment='{}' output='{}'",
        command,
        summarize_detect_log_text(expected_prompt, 80),
        summarize_detect_log_text(last_fragment, 120),
        summarize_detect_log_text(raw_output, 160)
    )
}

#[derive(Debug, Default, Clone)]
struct DetectProbeCache {
    outputs: HashMap<String, String>,
}

impl DetectProbeCache {
    fn get(&self, command: &str) -> Option<&str> {
        self.outputs.get(command).map(String::as_str)
    }

    fn insert(&mut self, command: impl Into<String>, output: impl Into<String>) {
        self.outputs.insert(command.into(), output.into());
    }

    fn into_map(self) -> HashMap<String, String> {
        self.outputs
    }
}

impl SshConnectionManager {
    pub(crate) async fn collect_detect_snapshot(
        &self,
        request: &DetectRequest,
        context: &ExecutionContext,
        probe_commands: &[String],
    ) -> Result<DetectSnapshot, ConnectError> {
        collect_detect_snapshot(request, &context.security_options, probe_commands).await
    }
}

async fn collect_detect_snapshot(
    request: &DetectRequest,
    security_options: &ConnectionSecurityOptions,
    probe_commands: &[String],
) -> Result<DetectSnapshot, ConnectError> {
    let device_addr = request.device_addr();
    debug!(
        "autodetect opening temporary SSH shell target={} port={} probe_commands={}",
        request.addr,
        request.port,
        probe_commands.len()
    );
    let config = Config {
        preferred: security_options.preferred(),
        inactivity_timeout: Some(Duration::from_secs(60)),
        ..Default::default()
    };
    let transport_auth = request.auth.to_transport().await?;

    let client = Client::connect_with_config(
        (request.addr.as_str(), request.port),
        &request.user,
        transport_auth,
        security_options.server_check.clone(),
        config,
    )
    .await
    .map_err(|error| ConnectError::ssh2_stage("autodetect_connect", device_addr.clone(), error))?;

    let mut channel = client.get_channel().await.map_err(|error| {
        ConnectError::ssh2_stage("autodetect_open_channel", device_addr.clone(), error)
    })?;
    channel
        .request_pty(false, "xterm", 800, 600, 0, 0, &[])
        .await
        .map_err(|error| {
            ConnectError::russh_stage("autodetect_request_pty", device_addr.clone(), error)
        })?;
    channel.request_shell(false).await.map_err(|error| {
        ConnectError::russh_stage("autodetect_request_shell", device_addr.clone(), error)
    })?;

    let mut initial_raw = String::new();
    let prompt = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            match channel.wait().await {
                Some(ChannelMsg::Data { ref data }) => {
                    if let Ok(chunk) = std::str::from_utf8(data) {
                        initial_raw.push_str(chunk);
                        let normalized = normalize_detect_output(&initial_raw);
                        let fragment = latest_terminal_fragment(&normalized).trim_end();
                        if !fragment.is_empty() && looks_like_shell_prompt(fragment) {
                            debug!(
                                "autodetect detected initial prompt='{}' initial_output='{}'",
                                summarize_detect_log_text(fragment, 80),
                                summarize_detect_log_text(&normalized, 120)
                            );
                            return Ok(fragment.to_string());
                        }
                    }
                }
                Some(ChannelMsg::Eof) | None => {
                    return Err(ConnectError::channel_disconnect_stage(
                        "autodetect_wait_initial_prompt",
                        device_addr.clone(),
                    ));
                }
                _ => {}
            }
        }
    })
    .await
    .map_err(|_| {
        ConnectError::InitTimeout("autodetect waiting for initial prompt".to_string())
    })??;

    let mut cache = DetectProbeCache::default();
    for command in probe_commands {
        if cache.get(command).is_some() {
            trace!("autodetect reusing cached probe command='{}'", command);
            continue;
        }

        debug!("autodetect running probe command='{}'", command);
        channel
            .data(format!("{command}\n").as_bytes())
            .await
            .map_err(|error| {
                ConnectError::russh_stage(
                    "autodetect_send_probe_command",
                    device_addr.clone(),
                    error,
                )
            })?;
        let mut raw = String::new();
        let output = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                match channel.wait().await {
                    Some(ChannelMsg::Data { ref data }) => {
                        if let Ok(chunk) = std::str::from_utf8(data) {
                            raw.push_str(chunk);
                            let normalized = normalize_detect_output(&raw);
                            let fragment = latest_terminal_fragment(&normalized).trim_end();
                            trace!(
                                "autodetect probe waiting command='{}' chunk='{}' latest_fragment='{}' target_prompt='{}'",
                                command,
                                summarize_detect_log_text(chunk, 120),
                                summarize_detect_log_text(fragment, 120),
                                summarize_detect_log_text(&prompt, 80)
                            );
                            if looks_like_pager_prompt(fragment) {
                                debug!(
                                    "autodetect probe encountered pager command='{}' pager='{}'; sending space",
                                    command,
                                    summarize_detect_log_text(fragment, 80)
                                );
                                channel.data(&b" "[..]).await.map_err(|error| {
                                    ConnectError::russh_stage(
                                        "autodetect_send_pager_continue",
                                        device_addr.clone(),
                                        error,
                                    )
                                })?;
                                continue;
                            }
                            if fragment == prompt {
                                let stripped =
                                    strip_detect_echo_and_prompt(&normalized, command, &prompt);
                                debug!(
                                    "autodetect probe completed command='{}' output='{}'",
                                    command,
                                    summarize_detect_log_text(&stripped, 120)
                                );
                                return Ok(stripped);
                            }
                        }
                    }
                    Some(ChannelMsg::Eof) | None => {
                        return Err(ConnectError::channel_disconnect_stage(
                            "autodetect_wait_probe_output",
                            device_addr.clone(),
                        ));
                    }
                    _ => {}
                }
            }
        })
        .await
        .map_err(|_| {
            let normalized = normalize_detect_output(&raw);
            let last_fragment = latest_terminal_fragment(&normalized).trim_end().to_string();
            let message = build_probe_timeout_message(command, &prompt, &last_fragment, &normalized);
            debug!("{}", message);
            ConnectError::ExecTimeout(message)
        })??;

        cache.insert(command.clone(), output);
    }

    let _ = channel.close().await;
    debug!(
        "autodetect snapshot ready prompt='{}' cached_probes={}",
        summarize_detect_log_text(&prompt, 80),
        cache.outputs.len()
    );

    Ok(DetectSnapshot {
        initial_output: normalize_detect_output(&initial_raw),
        initial_prompt: prompt,
        probe_outputs: cache.into_map(),
    })
}

fn strip_detect_echo_and_prompt(output: &str, command: &str, prompt: &str) -> String {
    let mut lines = output
        .trim_start_matches(['\r', '\n'])
        .lines()
        .map(str::to_string)
        .collect::<Vec<_>>();

    while let Some(last) = lines.last() {
        if last.trim().is_empty() {
            lines.pop();
            continue;
        }

        if last.trim() == prompt.trim() {
            lines.pop();

            while let Some(prefix) = lines.last() {
                if should_strip_prompt_prefix(prefix) {
                    lines.pop();
                } else {
                    break;
                }
            }
        }

        break;
    }

    while let Some(first) = lines.first() {
        let trimmed = first.trim();
        if trimmed.is_empty() {
            lines.remove(0);
            continue;
        }

        if trimmed == command.trim() {
            lines.remove(0);
            continue;
        }

        let prompt_with_command = format!("{}{}", prompt.trim_end(), command);
        if trimmed == prompt_with_command.trim() {
            lines.remove(0);
        }

        break;
    }

    lines.join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_probe_cache_deduplicates_commands() {
        let mut cache = DetectProbeCache::default();
        cache.insert("show version", "Cisco IOS XE Software");

        assert_eq!(cache.get("show version"), Some("Cisco IOS XE Software"));
    }

    #[test]
    fn detect_initial_output_normalization_reuses_shared_terminal_cleanup() {
        let raw = "Welcome\r\n\u{1b}[1mrouter#\u{1b}[0m ";
        let normalized = normalize_detect_output(raw);
        assert_eq!(normalized, "Welcome\nrouter# ");
    }

    #[test]
    fn prompt_heuristic_accepts_common_network_and_shell_suffixes() {
        assert!(looks_like_shell_prompt("router#"));
        assert!(looks_like_shell_prompt("router>"));
        assert!(looks_like_shell_prompt("user@host:~$"));
        assert!(looks_like_shell_prompt("<huawei>"));
    }

    #[test]
    fn pager_prompt_heuristic_accepts_common_more_markers() {
        assert!(looks_like_pager_prompt("<--- More --->"));
        assert!(looks_like_pager_prompt("--More--"));
        assert!(looks_like_pager_prompt("---- More ----"));
        assert!(looks_like_pager_prompt("-- More --"));
    }

    #[test]
    fn pager_prompt_heuristic_rejects_normal_output_lines() {
        assert!(!looks_like_pager_prompt(
            "Cisco Adaptive Security Appliance Software Version 9.8(1)"
        ));
        assert!(!looks_like_pager_prompt("ciscoasa-3>"));
    }

    #[test]
    fn strip_detect_echo_and_prompt_removes_prompt_prefixed_echo_and_trailing_prefix() {
        let output = "router#show version\nCisco IOS XE Software\n[edit]\nrouter# ";
        let stripped = strip_detect_echo_and_prompt(output, "show version", "router#");

        assert_eq!(stripped, "Cisco IOS XE Software");
    }

    #[test]
    fn probe_timeout_message_includes_waiting_context() {
        let message = build_probe_timeout_message(
            "show version",
            "ciscoasa-3>",
            "Type help or '?' for a list of available commands.\nciscoasa-3#",
            "show version\nCisco Adaptive Security Appliance Software Version 9.18(4)\n",
        );

        assert!(message.contains("autodetect probe timeout: show version"));
        assert!(message.contains("waiting_for_prompt='ciscoasa-3>'"));
        assert!(message.contains(
            "last_fragment='Type help or '?' for a list of available commands. ciscoasa-3#'"
        ));
        assert!(message.contains(
            "output='show version Cisco Adaptive Security Appliance Software Version 9.18(4)'"
        ));
    }
}
