//! Run virtual SSH devices from the rneter testkit as standalone servers.
//!
//! Point an external process (or a plain `ssh` client) at the printed
//! address to interact with the scripted CLI.
//!
//! Usage:
//!
//! ```text
//! # List every built-in device persona
//! cargo run --example virtual_device --features testkit -- --list
//!
//! # Run one virtual device (port defaults to an ephemeral one)
//! cargo run --example virtual_device --features testkit -- cisco_ios 2201
//!
//! # Run a fleet: one virtual device per built-in template
//! cargo run --example virtual_device --features testkit -- --all 2200
//!
//! # Run a self-defined virtual device (custom template + persona)
//! cargo run --example virtual_device --features testkit -- --custom 2300
//! ```

use rneter::device::{DeviceHandlerConfig, input_rule, prompt_rule, transition_rule};
use rneter::testkit::{DEFAULT_ENABLE_PASSWORD, DevicePersona, FakeSshDevice, builtin_personas};

fn print_device(device: &FakeSshDevice) {
    let persona = device.persona();
    println!(
        "  {:<18} ssh -p {:<5} {}@127.0.0.1  (password: {}{})",
        persona.name,
        device.port(),
        persona.username,
        persona.password,
        persona
            .enable_password
            .as_ref()
            .map(|p| format!(", enable: {p}"))
            .unwrap_or_default()
    );
}

/// A self-defined device type: custom prompts, transitions, and error style.
fn custom_persona() -> DevicePersona {
    let config = DeviceHandlerConfig {
        prompt: vec![
            prompt_rule("Login", &[r"^acme>\s*$"]),
            prompt_rule("Enable", &[r"^acme#\s*$"]),
            prompt_rule("Config", &[r"^acme\(conf\)#\s*$"]),
        ],
        write: vec![input_rule(
            "EnablePassword",
            true,
            "EnablePassword",
            false,
            &[r"^Admin password:\s*$"],
        )],
        error_regex: vec![r"^ACME-ERR: .+$".to_string()],
        edges: vec![
            transition_rule("Login", "admin", "Enable", false, false),
            transition_rule("Enable", "conf", "Config", false, false),
            transition_rule("Config", "quit", "Enable", true, false),
            transition_rule("Enable", "quit", "Login", true, false),
        ],
        ..Default::default()
    };

    DevicePersona::for_config(
        "acme_os",
        config,
        "login",
        &[
            ("login", "acme>"),
            ("enable", "acme#"),
            ("config", "acme(conf)#"),
        ],
    )
    .with_challenge("admin", "Admin password: ", DEFAULT_ENABLE_PASSWORD)
    .with_error_reply("ACME-ERR: no such command")
    .with_enable_password(DEFAULT_ENABLE_PASSWORD)
}

fn print_usage() {
    println!("usage: virtual_device --list");
    println!("       virtual_device <template> [port]");
    println!("       virtual_device --all [base_port]");
    println!("       virtual_device --custom [port]");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let port_arg = |idx: usize| args.get(idx).and_then(|p| p.parse::<u16>().ok());

    match args.first().map(String::as_str) {
        Some("--list") => {
            println!("built-in device personas ({}):", builtin_personas()?.len());
            for persona in builtin_personas()? {
                let states: Vec<&str> = persona
                    .config
                    .prompt
                    .iter()
                    .map(|rule| rule.state.as_str())
                    .collect();
                println!("  {:<18} states: {}", persona.name, states.join(" -> "));
            }
        }
        Some("--all") => {
            let base_port = port_arg(1).unwrap_or(2200);
            let mut devices = Vec::new();
            for (offset, persona) in builtin_personas()?.into_iter().enumerate() {
                let port = base_port
                    .checked_add(offset as u16)
                    .ok_or("base port too high: the fleet does not fit below port 65536")?;
                devices.push(FakeSshDevice::spawn_on(persona, ("127.0.0.1", port)).await?);
            }
            println!(
                "running {} virtual devices (Ctrl-C to stop):",
                devices.len()
            );
            for device in &devices {
                print_device(device);
            }
            std::future::pending::<()>().await;
        }
        Some("--custom") => {
            let port = port_arg(1).unwrap_or(2300);
            let device = FakeSshDevice::spawn_on(custom_persona(), ("127.0.0.1", port)).await?;
            println!("running self-defined virtual device (Ctrl-C to stop):");
            print_device(&device);
            println!(
                "  try: admin -> {DEFAULT_ENABLE_PASSWORD} -> conf -> any command -> make-error"
            );
            std::future::pending::<()>().await;
        }
        Some(template) => {
            let persona = DevicePersona::builtin(template)?;
            let device = match port_arg(1) {
                Some(port) => FakeSshDevice::spawn_on(persona, ("127.0.0.1", port)).await?,
                None => FakeSshDevice::spawn(persona).await?,
            };
            println!("running virtual device (Ctrl-C to stop):");
            print_device(&device);
            std::future::pending::<()>().await;
        }
        None => print_usage(),
    }

    Ok(())
}
