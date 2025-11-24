#[path = "../socket.rs"]
mod socket;

use prism::ipc::{
    ClientInfoPayload, CommandRequest, CustomPropertyPayload, HelpEntry, RoutingUpdateAck,
    RpcResponse,
};
use serde::de::DeserializeOwned;
use serde_json::{self, Value};
use std::env;
use std::io::{self, BufReader, Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;

fn main() {
    let mut args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        let _ = handle_help();
        return;
    }

    let command = args.remove(0);
    let result = match command.as_str() {
        "set" => handle_set(args),
        "list" => handle_list(),
        "clients" => handle_clients(),
        "repl" => run_repl(),
        "help" | "--help" | "-h" => handle_help(),
        other => {
            eprintln!("Unknown command '{}'.", other);
            display_help_entries(&fallback_help_entries());
            Ok(())
        }
    };

    if let Err(err) = result {
        eprintln!("prism: {}", err);
        std::process::exit(1);
    }
}

fn handle_set(args: Vec<String>) -> Result<(), String> {
    if args.len() < 2 {
        return Err("Usage: prism set <PID> <OFFSET>".to_string());
    }

    let pid: i32 = args[0]
        .parse()
        .map_err(|_| "PID must be an integer".to_string())?;
    let offset: u32 = args[1]
        .parse()
        .map_err(|_| "OFFSET must be a non-negative integer".to_string())?;
    execute_set(pid, offset)
}

fn handle_list() -> Result<(), String> {
    execute_list()
}

fn handle_clients() -> Result<(), String> {
    execute_clients()
}
fn handle_help() -> Result<(), String> {
    match fetch_help_entries() {
        Ok((message, entries)) => {
            if let Some(msg) = message {
                println!("{}", msg);
            }
            display_help_entries(&entries);
        }
        Err(err) => {
            eprintln!("prism: {}", err);
            display_help_entries(&fallback_help_entries());
        }
    }
    Ok(())
}

fn run_repl() -> Result<(), String> {
    println!("Prism control REPL (commands are routed via prismd). Type 'help' for commands.");
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        stdout
            .write_all(b"> ")
            .and_then(|_| stdout.flush())
            .map_err(|err| err.to_string())?;

        let mut line = String::new();
        match stdin.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(err) => return Err(err.to_string()),
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if line.eq_ignore_ascii_case("exit") || line.eq_ignore_ascii_case("quit") {
            break;
        }

        if line.eq_ignore_ascii_case("help") {
            if let Err(err) = execute_command(CommandRequest::Help) {
                eprintln!("prism: {}", err);
            }
            continue;
        }

        if line.starts_with('{') {
            match send_raw_payload(line) {
                Ok(response) => print_response(&response),
                Err(err) => eprintln!("prism: {}", err),
            }
            continue;
        }

        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.is_empty() {
            continue;
        }

        let request = match build_command_from_tokens(tokens[0], &tokens[1..]) {
            Ok(req) => req,
            Err(err) => {
                eprintln!("prism: {}", err);
                continue;
            }
        };

        if let Err(err) = execute_command(request) {
            eprintln!("prism: {}", err);
        }
    }

    Ok(())
}

fn execute_command(request: CommandRequest) -> Result<(), String> {
    match request {
        CommandRequest::Set { pid, offset } => execute_set(pid, offset),
        CommandRequest::List => execute_list(),
        CommandRequest::Clients => execute_clients(),
        CommandRequest::Help => handle_help(),
        CommandRequest::Quit | CommandRequest::Exit => {
            Err("terminating prismd via CLI is not supported".to_string())
        }
    }
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

    println!("Active Prism clients ({}):", clients.len());
    for entry in clients {
        let name = entry.process_name.as_deref().unwrap_or("<unknown>");
        println!(
            "    pid={} ({}) client_id={} offset={}",
            entry.pid, name, entry.client_id, entry.channel_offset
        );
    }
    Ok(())
}

fn build_command_from_tokens(command: &str, args: &[&str]) -> Result<CommandRequest, String> {
    match command {
        "set" => {
            if args.len() < 2 {
                return Err("usage: set <PID> <OFFSET>".to_string());
            }
            let pid = args[0]
                .parse::<i32>()
                .map_err(|_| "PID must be an integer".to_string())?;
            let offset = args[1]
                .parse::<u32>()
                .map_err(|_| "OFFSET must be a non-negative integer".to_string())?;
            Ok(CommandRequest::Set { pid, offset })
        }
        "list" => {
            if !args.is_empty() {
                return Err("list takes no arguments".to_string());
            }
            Ok(CommandRequest::List)
        }
        "clients" => {
            if !args.is_empty() {
                return Err("clients takes no arguments".to_string());
            }
            Ok(CommandRequest::Clients)
        }
        "help" => Ok(CommandRequest::Help),
        other => Err(format!("unknown command '{}'; try 'help'", other)),
    }
}

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

fn fetch_help_entries() -> Result<(Option<String>, Vec<HelpEntry>), String> {
    let response = send_request(&CommandRequest::Help)?;
    let parsed: RpcResponse<Vec<HelpEntry>> = parse_response(&response)?;
    extract_success(parsed)
}

fn display_help_entries(entries: &[HelpEntry]) {
    println!("Usage: prism <command> [args]\n");
    println!("Commands:");
    for entry in entries {
        println!(
            "  {:<12} {:<22} {}",
            entry.command, entry.usage, entry.description
        );
    }
}

fn fallback_help_entries() -> Vec<HelpEntry> {
    vec![
        HelpEntry::new("list", "list", "Show driver properties via prismd"),
        HelpEntry::new("clients", "clients", "Show active Prism clients via prismd"),
        HelpEntry::new(
            "set",
            "set <PID> <OFFSET>",
            "Send routing update (relayed by prismd)",
        ),
        HelpEntry::new(
            "repl",
            "repl",
            "Start interactive shell (commands go to prismd)",
        ),
        HelpEntry::new("help", "help", "Show this help message"),
    ]
}

fn print_response(raw: &str) {
    match serde_json::from_str::<Value>(raw) {
        Ok(value) => {
            let pretty = serde_json::to_string_pretty(&value).unwrap_or_else(|_| raw.to_string());
            println!("{}", pretty);
        }
        Err(err) => {
            eprintln!("prism: invalid JSON response: {}", err);
            println!("{}", raw);
        }
    }
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
