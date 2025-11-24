#![allow(clippy::missing_safety_doc)]

#[path = "../host.rs"]
mod host;

#[path = "../socket.rs"]
mod socket;

use coreaudio_sys::*;
use host::{
    fetch_client_list, find_prism_device, read_custom_property_info, send_rout_update, ClientEntry,
    K_AUDIO_PRISM_PROPERTY_CLIENT_LIST,
};
use prism::ipc::{
    ClientInfoPayload, CommandRequest, CustomPropertyPayload, HelpEntry, RoutingUpdateAck,
    RpcResponse,
};
use serde::Serialize;
use std::env;
use std::ffi::{c_void, CStr};
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::{self, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

struct CliOptions {
    daemonize: bool,
    daemon_child: bool,
    show_help: bool,
    forward_args: Vec<String>,
}

static CLIENT_LIST: Mutex<Vec<ClientEntry>> = Mutex::new(Vec::new());

fn json_response<T>(status: &str, message: Option<String>, data: Option<T>) -> String
where
    T: Serialize,
{
    let payload = RpcResponse {
        status: status.to_string(),
        message,
        data,
    };
    let serialized = serde_json::to_string(&payload).unwrap_or_else(|err| {
        serde_json::to_string(&RpcResponse::<serde_json::Value> {
            status: "error".to_string(),
            message: Some(format!("failed to serialize response: {}", err)),
            data: None,
        })
        .unwrap()
    });
    format!("{}\n", serialized)
}

fn json_success_with_data<T>(data: T) -> String
where
    T: Serialize,
{
    json_response("ok", None, Some(data))
}

fn json_success_with_message_and_data<T>(message: String, data: T) -> String
where
    T: Serialize,
{
    json_response("ok", Some(message), Some(data))
}

fn json_error(message: String) -> String {
    json_response::<serde_json::Value>("error", Some(message), None)
}

fn help_payload() -> Vec<HelpEntry> {
    vec![
        HelpEntry::new(
            "set",
            "set <PID> <OFFSET>",
            "Send a routing update to map PID to channel offset",
        ),
        HelpEntry::new(
            "list",
            "list",
            "List custom driver properties exposed by Prism",
        ),
        HelpEntry::new(
            "clients",
            "clients",
            "Show active Prism clients with routing offsets",
        ),
    ]
}

fn parse_cli_options() -> CliOptions {
    let mut options = CliOptions {
        daemonize: false,
        daemon_child: false,
        show_help: false,
        forward_args: Vec::new(),
    };

    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--daemonize" | "-d" => options.daemonize = true,
            "--daemon-child" => options.daemon_child = true,
            "--help" | "-h" => options.show_help = true,
            _ => options.forward_args.push(arg),
        }
    }

    options
}

fn print_usage() {
    println!(
        "Usage: prismd [OPTIONS]\n\nOptions:\n  -d, --daemonize    Run as a background process\n  -h, --help         Show this help message"
    );
}

fn spawn_daemon_child(args: &[String]) -> Result<u32, String> {
    let exe = env::current_exe().map_err(|err| err.to_string())?;

    let mut child_args = Vec::with_capacity(args.len() + 1);
    child_args.extend(args.iter().cloned());
    child_args.push("--daemon-child".to_string());

    let child = Command::new(exe)
        .args(&child_args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| err.to_string())?;

    Ok(child.id())
}

fn main() {
    let options = parse_cli_options();

    if options.daemon_child {
        run_daemon();
        return;
    }

    if options.show_help {
        print_usage();
        return;
    }

    if options.daemonize {
        match spawn_daemon_child(&options.forward_args) {
            Ok(pid) => {
                println!("prismd started in background (pid={})", pid);
                return;
            }
            Err(err) => {
                eprintln!("[prismd] Failed to daemonize: {}", err);
                process::exit(1);
            }
        }
    }

    if !options.forward_args.is_empty() {
        eprintln!(
            "[prismd] Unknown arguments: {}",
            options.forward_args.join(" ")
        );
        process::exit(2);
    }

    run_daemon();
}

struct ClientListContext {
    device_id: AudioObjectID,
}

unsafe extern "C" fn client_list_listener(
    _: AudioObjectID,
    _: UInt32,
    _: *const AudioObjectPropertyAddress,
    client_data: *mut c_void,
) -> OSStatus {
    if client_data.is_null() {
        return 0;
    }

    let context = &*(client_data as *mut ClientListContext);
    if let Err(err) = handle_client_list_update(context.device_id) {
        eprintln!("[prismd] Failed to refresh client list: {}", err);
    }

    0
}

fn handle_client_list_update(device_id: AudioObjectID) -> Result<(), String> {
    let clients = fetch_client_list(device_id)?;

    {
        let mut cache = CLIENT_LIST.lock().expect("client list mutex poisoned");
        *cache = clients.clone();
    }

    println!("[prismd] Client list updated ({} entries)", clients.len());
    for entry in &clients {
        let process_name =
            resolve_process_name(entry.pid).unwrap_or_else(|| "<unknown>".to_string());
        println!(
            "    pid={} ({}) client_id={} offset={}",
            entry.pid, process_name, entry.client_id, entry.channel_offset
        );
    }

    Ok(())
}

fn register_client_list_listener(device_id: AudioObjectID) -> Result<(), String> {
    let address = AudioObjectPropertyAddress {
        mSelector: K_AUDIO_PRISM_PROPERTY_CLIENT_LIST,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster,
    };

    let context = Box::new(ClientListContext { device_id });
    let context_ptr = Box::into_raw(context);
    let status = unsafe {
        AudioObjectAddPropertyListener(
            device_id,
            &address,
            Some(client_list_listener),
            context_ptr as *mut _,
        )
    };

    if status != 0 {
        unsafe {
            drop(Box::from_raw(context_ptr));
        }
        return Err(format!(
            "AudioObjectAddPropertyListener('clnt') failed with status {}",
            status
        ));
    }

    Ok(())
}

fn start_ipc_server(device_id: AudioObjectID) -> io::Result<()> {
    if let Err(err) = fs::remove_file(socket::PRISM_SOCKET_PATH) {
        if err.kind() != io::ErrorKind::NotFound {
            eprintln!(
                "[prismd] Warning: failed to remove existing socket {}: {}",
                socket::PRISM_SOCKET_PATH,
                err
            );
        }
    }

    let listener = UnixListener::bind(socket::PRISM_SOCKET_PATH)?;
    if let Err(err) =
        fs::set_permissions(socket::PRISM_SOCKET_PATH, fs::Permissions::from_mode(0o660))
    {
        eprintln!(
            "[prismd] Warning: failed to set permissions on {}: {}",
            socket::PRISM_SOCKET_PATH,
            err
        );
    }

    thread::Builder::new()
        .name("prismd-ipc".to_string())
        .spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => handle_ipc_connection(stream, device_id),
                    Err(err) => eprintln!("[prismd] IPC accept error: {}", err),
                }
            }
        })?;

    Ok(())
}

fn handle_ipc_connection(stream: UnixStream, device_id: AudioObjectID) {
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(cloned) => cloned,
        Err(err) => {
            eprintln!("[prismd] Failed to clone IPC stream: {}", err);
            return;
        }
    });

    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(0) => return,
        Ok(_) => {}
        Err(err) => {
            eprintln!("[prismd] Failed to read IPC command: {}", err);
            return;
        }
    }

    let response = handle_ipc_command(line.trim(), device_id);

    if let Err(err) = write_all_and_flush(stream, response.as_bytes()) {
        eprintln!("[prismd] Failed to write IPC response: {}", err);
    }
}

fn write_all_and_flush(mut stream: UnixStream, bytes: &[u8]) -> io::Result<()> {
    stream.write_all(bytes)?;
    stream.flush()
}

fn handle_ipc_command(raw: &str, device_id: AudioObjectID) -> String {
    if raw.is_empty() {
        return json_error("empty command".to_string());
    }

    let request: CommandRequest = match serde_json::from_str(raw) {
        Ok(req) => req,
        Err(err) => return json_error(format!("invalid request: {}", err)),
    };

    match request {
        CommandRequest::Help => json_success_with_data(help_payload()),
        CommandRequest::Clients => match build_clients_payload(device_id) {
            Ok(payload) => json_success_with_data(payload),
            Err(err) => json_error(format!("failed to fetch clients: {}", err)),
        },
        CommandRequest::List => match build_custom_properties_payload(device_id) {
            Ok(payload) => json_success_with_data(payload),
            Err(err) => json_error(format!("failed to read custom properties: {}", err)),
        },
        CommandRequest::Set { pid, offset } => match send_rout_update(device_id, pid, offset) {
            Ok(()) => json_success_with_message_and_data(
                "routing update sent".to_string(),
                RoutingUpdateAck {
                    pid,
                    channel_offset: offset,
                },
            ),
            Err(err) => json_error(format!("failed to send routing update: {}", err)),
        },
        CommandRequest::Quit | CommandRequest::Exit => {
            json_error("terminating prismd via CLI is not supported".to_string())
        }
    }
}

fn build_clients_payload(device_id: AudioObjectID) -> Result<Vec<ClientInfoPayload>, String> {
    let clients = fetch_client_list(device_id)?;

    {
        let mut cache = CLIENT_LIST.lock().expect("client list mutex poisoned");
        *cache = clients.clone();
    }

    let payload = clients
        .into_iter()
        .map(|entry| ClientInfoPayload {
            pid: entry.pid,
            client_id: entry.client_id,
            channel_offset: entry.channel_offset,
            process_name: resolve_process_name(entry.pid),
        })
        .collect();

    Ok(payload)
}

fn resolve_process_name(pid: i32) -> Option<String> {
    if pid <= 0 {
        return None;
    }

    const BUF_SIZE: usize = 4096;
    let mut buffer = [0u8; BUF_SIZE];
    let ret = unsafe {
        libc::proc_pidpath(
            pid,
            buffer.as_mut_ptr() as *mut libc::c_void,
            BUF_SIZE as u32,
        )
    };

    if ret <= 0 {
        return None;
    }

    let cstr = unsafe { CStr::from_ptr(buffer.as_ptr() as *const libc::c_char) };
    let path = cstr.to_string_lossy();
    let name = path
        .rsplit('/')
        .next()
        .map(|segment| segment.to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| path.to_string());

    Some(name)
}

fn build_custom_properties_payload(
    device_id: AudioObjectID,
) -> Result<Vec<CustomPropertyPayload>, String> {
    let entries = read_custom_property_info(device_id)?;

    let payload = entries
        .into_iter()
        .map(|entry| CustomPropertyPayload {
            selector: entry.selector,
            property_data_type: entry.property_data_type,
            qualifier_data_type: entry.qualifier_data_type,
        })
        .collect();

    Ok(payload)
}

fn run_daemon() {
    println!("Prism Daemon (prismd) starting...");

    let device_id = match find_prism_device() {
        Ok(id) => id,
        Err(err) => {
            eprintln!("Prism driver not found: {}", err);
            return;
        }
    };

    println!("Found Prism Device ID: {}", device_id);

    match register_client_list_listener(device_id) {
        Ok(()) => {
            if let Err(err) = handle_client_list_update(device_id) {
                eprintln!("[prismd] Initial client list fetch failed: {}", err);
            }
        }
        Err(err) => {
            eprintln!("[prismd] Failed to register client list listener: {}", err);
            return;
        }
    }

    if let Err(err) = start_ipc_server(device_id) {
        eprintln!("[prismd] Failed to start IPC server: {}", err);
        return;
    }

    println!(
        "prismd is now monitoring the Prism driver (socket: {}). Press Ctrl+C to exit.",
        socket::PRISM_SOCKET_PATH
    );

    loop {
        thread::sleep(Duration::from_secs(60));
    }
}
