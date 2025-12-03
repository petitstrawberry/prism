#[path = "../socket.rs"]
mod socket;

use clap::{Parser, Subcommand};
use prism::ipc::{
    ClientInfoPayload, CommandRequest, CustomPropertyPayload, HelpEntry, RoutingUpdateAck,
    RpcResponse,
};
use serde::de::DeserializeOwned;
use serde_json::{self};
use std::collections::BTreeMap;
// std::env not required here (clap handles args)
use std::io::{BufReader, Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;

#[derive(Parser)]
#[command(name = "prism", about = "Prism control CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Send routing update to a PID
    #[command(about = "Send routing update to a PID")]
    Set {
        #[arg(value_name = "PID")]
        pid: i32,
        #[arg(value_name = "OFFSET|CH1-CH2")]
        offset: String,
    },
    /// List driver custom properties
    #[command(about = "List driver custom properties")]
    List,
    /// Show active Prism clients grouped by responsibility
    #[command(about = "Show active Prism clients grouped by responsibility")]
    Clients,
    /// List apps grouped by responsible process
    #[command(about = "List apps grouped by responsible process")]
    Apps,
    /// Set channel offset for all clients of an app
    #[command(about = "Set channel offset for all clients of an app")]
    SetApp {
        #[arg(value_name = "APP_NAME")]
        app_name: String,
        #[arg(value_name = "OFFSET|CH1-CH2")]
        offset: String,
    },
}

fn main() {
    let cli = Cli::parse();

    let res = match cli.command {
        Commands::Set { pid, offset } => handle_set(vec![pid.to_string(), offset]),
        Commands::List => handle_list(),
        Commands::Clients => handle_clients(),
        Commands::Apps => handle_apps(Vec::new()),
        Commands::SetApp { app_name, offset } => handle_set_app(vec![app_name, offset]),
    };

    if let Err(err) = res {
        eprintln!("prism: {}", err);
        std::process::exit(1);
    }
}

fn handle_apps(_args: Vec<String>) -> Result<(), String> {
    // The apps command retrieves data via the Apps request
    let response = send_request(&CommandRequest::Apps)?;
    let parsed: RpcResponse<Vec<ClientInfoPayload>> = parse_response(&response)?;
    let (_message, clients): (Option<String>, Vec<ClientInfoPayload>) = extract_success(parsed)?;

    use std::collections::BTreeMap;
    // Group by responsible process
    let mut groups: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    let mut ungrouped: Vec<u32> = Vec::new();
    for client in &clients {
        if let Some(name) = client
            .responsible_name
            .as_ref()
            .or(client.process_name.as_ref())
        {
            groups
                .entry(name.clone())
                .or_default()
                .push(client.channel_offset);
        } else {
            ungrouped.push(client.channel_offset);
        }
    }

    // Calculate the maximum app name width
    let mut max_name_len = 10;
    for name in groups.keys() {
        if name.len() > max_name_len {
            max_name_len = name.len();
        }
    }
    // Header
    println!(
        "{:<width$} | {:>16}",
        "App",
        "Channels",
        width = max_name_len
    );
    println!("{}-+-{}", "-".repeat(max_name_len), "-".repeat(16));
    // Display groups
    for (name, offsets) in groups.iter() {
        let mut offsets = offsets.clone();
        offsets.sort_unstable();
        offsets.dedup();
        let offset_str = offsets
            .iter()
            .map(|o| {
                let ch1 = o + 1;
                let ch2 = o + 2;
                format!("{}-{}ch", ch1, ch2)
            })
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "{:<width$} | {:>16}",
            name,
            offset_str,
            width = max_name_len
        );
    }
    // Display ungrouped
    if !ungrouped.is_empty() {
        let mut offsets = ungrouped.clone();
        offsets.sort_unstable();
        offsets.dedup();
        let offset_str = offsets
            .iter()
            .map(|o| {
                let ch1 = o * 2;
                let ch2 = o * 2 + 1;
                format!("{}-{}ch", ch1, ch2)
            })
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "{:<width$} | {:>16}",
            "(Ungrouped)",
            offset_str,
            width = max_name_len
        );
    }
    Ok(())
}

fn handle_set_app(args: Vec<String>) -> Result<(), String> {
    // set-app <APP_NAME> <OFFSET|CH1-CH2>
    // Accept app name containing spaces by treating the last arg as the offset
    if args.len() < 2 {
        return Err("Usage: prism set-app <APP_NAME> <OFFSET|CH1-CH2>".to_string());
    }
    let offset_arg = args.last().unwrap().to_string();
    let app_name = args[..args.len() - 1].join(" ");
    // Accept either numeric offset or channel range like "1-2"
    let offset: u32 = if let Some((ch1, ch2)) = parse_channel_range(&offset_arg) {
        if ch2 != ch1 + 1 {
            return Err("Channel range must be consecutive (e.g. 1-2, 3-4)".to_string());
        }
        if ch1 < 1 {
            return Err("Channel numbers must be >= 1".to_string());
        }
        ch1 - 1
    } else {
        offset_arg.parse().map_err(|_| {
            "OFFSET must be a non-negative integer or channel range (e.g. 1-2)".to_string()
        })?
    };
    // Delegate the app-level update to prismd (daemon) and display its result.
    let response = send_request(&CommandRequest::SetApp {
        app_name: app_name.clone(),
        offset,
    })?;
    let parsed: RpcResponse<Vec<RoutingUpdateAck>> = parse_response(&response)?;
    let (_message, results): (Option<String>, Vec<RoutingUpdateAck>) = extract_success(parsed)?;

    if results.is_empty() {
        println!("No clients found for app '{}'.", app_name);
    } else {
        let pids: Vec<String> = results.iter().map(|ack| ack.pid.to_string()).collect();
        println!(
            "Set offset={} for app '{}' (pids: {})",
            offset,
            app_name,
            pids.join(", ")
        );
    }
    Ok(())
}

fn handle_set(args: Vec<String>) -> Result<(), String> {
    if args.len() < 2 {
        return Err("Usage: prism set <PID> <OFFSET|CH1-CH2>".to_string());
    }

    let pid: i32 = args[0]
        .parse()
        .map_err(|_| "PID must be an integer".to_string())?;

    // Accept either offset or CH1-CH2 format
    let offset: u32 = if let Some((ch1, ch2)) = parse_channel_range(&args[1]) {
        // offset = ch1 - 1
        if ch2 != ch1 + 1 {
            return Err("Channel range must be consecutive (e.g. 1-2, 2-3)".to_string());
        }
        if ch1 < 1 {
            return Err("Channel numbers must be >= 1".to_string());
        }
        ch1 - 1
    } else {
        args[1].parse().map_err(|_| {
            "OFFSET must be a non-negative integer or channel range (e.g. 1-2)".to_string()
        })?
    };
    execute_set(pid, offset)
}

fn handle_list() -> Result<(), String> {
    execute_list()
}

fn handle_clients() -> Result<(), String> {
    execute_clients()
}

fn execute_set(pid: i32, offset: u32) -> Result<(), String> {
    let response = send_request(&CommandRequest::Set { pid, offset })?;
    let parsed: RpcResponse<RoutingUpdateAck> = parse_response(&response)?;
    let (message, ack): (Option<String>, RoutingUpdateAck) = extract_success(parsed)?;
    if let Some(msg) = message {
        println!("{} (pid={} offset={})", msg, ack.pid, ack.channel_offset);
    } else {
        println!(
            "Routing update sent: pid={} offset={}",
            ack.pid, ack.channel_offset
        );
    }
    Ok(())
}

fn execute_list() -> Result<(), String> {
    let response = send_request(&CommandRequest::List)?;
    let parsed: RpcResponse<Vec<CustomPropertyPayload>> = parse_response(&response)?;
    let (message, entries): (Option<String>, Vec<CustomPropertyPayload>) = extract_success(parsed)?;

    if let Some(msg) = message {
        println!("{}", msg);
    }

    if entries.is_empty() {
        println!("No custom properties reported by Prism driver.");
        return Ok(());
    }

    println!("Custom properties:");
    for (index, entry) in entries.iter().enumerate() {
        let (selector_text, selector_hex) = format_fourcc(entry.selector);
        let (type_text, type_hex) = format_fourcc(entry.property_data_type);
        println!(
            "  [{}] selector='{}' (0x{:08X}) type='{}' (0x{:08X}) qualifier=0x{:08X}",
            index, selector_text, selector_hex, type_text, type_hex, entry.qualifier_data_type
        );
    }
    Ok(())
}

fn execute_clients() -> Result<(), String> {
    let response = send_request(&CommandRequest::Clients)?;
    let parsed: RpcResponse<Vec<ClientInfoPayload>> = parse_response(&response)?;
    let (message, clients): (Option<String>, Vec<ClientInfoPayload>) = extract_success(parsed)?;

    if let Some(msg) = message {
        println!("{}", msg);
    }

    if clients.is_empty() {
        println!("No active Prism clients.");
        return Ok(());
    }

    let mut groups: BTreeMap<i32, (Option<String>, Vec<ClientInfoPayload>)> = BTreeMap::new();
    let mut ungrouped: Vec<ClientInfoPayload> = Vec::new();

    for entry in clients {
        if let Some(pid) = entry.responsible_pid {
            let display_name = entry
                .responsible_name
                .clone()
                .or_else(|| entry.process_name.clone());
            let group = groups
                .entry(pid)
                .or_insert_with(|| (display_name.clone(), Vec::new()));
            if group.0.is_none() && display_name.is_some() {
                group.0 = display_name.clone();
            }
            group.1.push(entry);
        } else {
            ungrouped.push(entry);
        }
    }

    let total_clients: usize = groups
        .values()
        .map(|(_, members)| members.len())
        .sum::<usize>()
        + ungrouped.len();

    println!(
        "Active Prism clients grouped by responsibility ({} client{})",
        total_clients,
        if total_clients == 1 { "" } else { "s" }
    );

    for (pid, (name, members)) in groups.iter_mut() {
        members.sort_by(|a, b| a.pid.cmp(&b.pid).then(a.client_id.cmp(&b.client_id)));
        let display_name = name.as_deref().unwrap_or("<unknown>");
        println!(
            "  Responsible pid={} ({}) [{} member{}]",
            pid,
            display_name,
            members.len(),
            if members.len() == 1 { "" } else { "s" }
        );

        for client in members {
            let proc_name = client.process_name.as_deref().unwrap_or("<unknown>");
            let marker = if Some(*pid) == client.responsible_pid && client.pid == *pid {
                "*"
            } else {
                "-"
            };
            println!(
                "    {} pid={} ({}) client_id={} offset={}",
                marker, client.pid, proc_name, client.client_id, client.channel_offset
            );
        }
    }

    if !ungrouped.is_empty() {
        ungrouped.sort_by(|a, b| a.pid.cmp(&b.pid).then(a.client_id.cmp(&b.client_id)));
        println!("  Ungrouped clients ({}):", ungrouped.len());
        for client in ungrouped {
            let proc_name = client.process_name.as_deref().unwrap_or("<unknown>");
            println!(
                "    - pid={} ({}) client_id={} offset={}",
                client.pid, proc_name, client.client_id, client.channel_offset
            );
        }
    }

    if !groups.is_empty() {
        println!("  ('*' marks the responsible process owning the group)");
    }
    Ok(())
}

// Token-based command builder removed with REPL.
fn send_request(request: &CommandRequest) -> Result<String, String> {
    let payload = serde_json::to_string(request)
        .map_err(|err| format!("failed to encode request: {}", err))?;
    send_raw_payload(&payload)
}
fn send_raw_payload(payload: &str) -> Result<String, String> {
    let mut stream = UnixStream::connect(socket::PRISM_SOCKET_PATH)
        .map_err(|err| format!("failed to connect to prismd: {}", err))?;

    stream
        .write_all(payload.as_bytes())
        .and_then(|_| stream.write_all(b"\n"))
        .and_then(|_| stream.flush())
        .map_err(|err| format!("failed to send command: {}", err))?;

    if let Err(err) = stream.shutdown(Shutdown::Write) {
        eprintln!("prism: warning: failed to half-close socket: {}", err);
    }

    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader
        .read_to_string(&mut response)
        .map_err(|err| format!("failed to read response: {}", err))?;

    Ok(response)
}

#[allow(dead_code)]
fn fetch_help_entries() -> Result<(Option<String>, Vec<HelpEntry>), String> {
    let response = send_request(&CommandRequest::Help)?;
    let parsed: RpcResponse<Vec<HelpEntry>> = parse_response(&response)?;
    extract_success(parsed)
}

#[allow(dead_code)]
fn display_help_entries(entries: &[HelpEntry]) {
    println!("Usage: prism <command> [args]\n");
    println!("Commands:");

    // Determine column widths (but cap usage width)
    let mut cmd_w = 6usize; // minimum
    let mut usage_w = 12usize; // minimum
    for e in entries {
        if e.command.len() > cmd_w {
            cmd_w = e.command.len();
        }
        if e.usage.len() > usage_w {
            usage_w = e.usage.len();
        }
    }
    if usage_w > 36 {
        usage_w = 36;
    }

    // Determine description wrap width based on terminal width (env COLUMNS) if available.
    let term_width: usize = std::env::var("COLUMNS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(80);

    // Reserve space for command and usage columns and margins.
    let reserved = cmd_w + usage_w + 6; // padding and separators
    let desc_w = if term_width > reserved + 20 {
        term_width - reserved
    } else {
        40usize
    };
    // Clamp desc width to reasonable bounds
    let desc_w = desc_w.clamp(20, 100);

    for entry in entries {
        // wrap description into lines
        let desc = entry.description.trim();
        let desc_lines = wrap_text(desc, desc_w);

        // print first line with command and usage
        let first_desc = desc_lines.first().map(|s| s.as_str()).unwrap_or("");
        println!(
            "  {usage:<usage_w$}  {desc}",
            usage = entry.usage,
            usage_w = usage_w,
            desc = first_desc
        );

        // print continuation lines for description
        for cont in desc_lines.iter().skip(1) {
            println!(
                "  {0:cmd_w$}  {1:usage_w$}  {cont}",
                "",
                "",
                cont = cont,
                cmd_w = cmd_w,
                usage_w = usage_w
            );
        }
    }
}

// Simple word-wrap: split on whitespace and build lines up to `width` characters.
#[allow(dead_code)]
fn wrap_text(s: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in s.split_whitespace() {
        if cur.is_empty() {
            cur.push_str(word);
        } else if cur.len() + 1 + word.len() <= width {
            cur.push(' ');
            cur.push_str(word);
        } else {
            lines.push(cur);
            cur = word.to_string();
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

#[allow(dead_code)]
fn fallback_help_entries() -> Vec<HelpEntry> {
    vec![
        HelpEntry::new("list", "list", "Show driver properties via prismd"),
        HelpEntry::new("clients", "clients", "Show active Prism clients via prismd"),
        HelpEntry::new(
            "set",
            "set <PID> <OFFSET|CH1-CH2>",
            "Send routing update (relayed by prismd). OFFSET or CH1-CH2 are accepted.",
        ),
        HelpEntry::new(
            "apps",
            "apps",
            "List active apps grouped by responsible process (shows channel ranges)",
        ),
        HelpEntry::new(
            "set-app",
            "set-app <APP_NAME> <OFFSET|CH1-CH2>",
            "Request prismd to set channel offset for all clients of APP_NAME",
        ),
        // repl removed; use subcommands instead
        HelpEntry::new("help", "help", "Show this help message"),
    ]
}

fn parse_response<T>(raw: &str) -> Result<RpcResponse<T>, String>
where
    T: DeserializeOwned,
{
    serde_json::from_str::<RpcResponse<T>>(raw)
        .map_err(|err| format!("invalid response from prismd: {}", err))
}

fn extract_success<T>(resp: RpcResponse<T>) -> Result<(Option<String>, T), String> {
    if resp.status != "ok" {
        return Err(resp.message.unwrap_or_else(|| "unknown error".to_string()));
    }

    let message = resp.message;
    resp.data
        .map(|data| (message, data))
        .ok_or_else(|| "missing data in response".to_string())
}

fn format_fourcc(value: u32) -> (String, u32) {
    let mut bytes = value.to_le_bytes();
    bytes.reverse();
    let text: String = bytes
        .iter()
        .map(|b| {
            let c = *b as char;
            if c.is_ascii_graphic() || c == ' ' {
                c
            } else {
                '?'
            }
        })
        .collect();
    (text, u32::from_be_bytes(bytes))
}

// Parse "1-2" or "2-3" style channel range, return (ch1, ch2) if valid, else None
fn parse_channel_range(s: &str) -> Option<(u32, u32)> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() == 2 {
        if let (Ok(ch1), Ok(ch2)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
            return Some((ch1, ch2));
        }
    }
    None
}
