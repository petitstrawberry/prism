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
use prism::ipc::{ClientInfoPayload, CommandRequest, CustomPropertyPayload, RoutingUpdateAck, RpcResponse};
use prism::process as procinfo;
use serde::Serialize;
use std::collections::HashSet;
use std::env;
use clap::Parser;
use std::ffi::c_void;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::{self, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "prismd", about = "Prism daemon controller")]
struct Opts {
    /// Run as a background process
    #[arg(short = 'd', long = "daemonize")]
    daemonize: bool,

    /// Internal flag used by the daemon child
    #[arg(long = "daemon-child")]
    daemon_child: bool,

    /// Forward unknown args (collected)
    #[arg(last = true)]
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

// daemon no longer provides a help payload; CLI serves local help.

// clap handles parsing and help printing for prismd

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
    let opts = Opts::parse();

    if opts.daemon_child {
        run_daemon();
        return;
    }

    if opts.daemonize {
        match spawn_daemon_child(&opts.forward_args) {
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

    if !opts.forward_args.is_empty() {
        eprintln!(
            "[prismd] Unknown arguments: {}",
            opts.forward_args.join(" ")
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
            procinfo::process_name(entry.pid).unwrap_or_else(|| "<unknown>".to_string());
        if let Some(identity) = procinfo::resolve_responsible_identity(entry.pid) {
            let responsible_name = identity
                .preferred_name()
                .unwrap_or_else(|| "<unknown>".to_string());
            if identity.pid != entry.pid {
                println!(
                    "    pid={} ({}) client_id={} offset={} -> responsible pid={} ({})",
                    entry.pid,
                    process_name,
                    entry.client_id,
                    entry.channel_offset,
                    identity.pid,
                    responsible_name
                );
            } else {
                println!(
                    "    pid={} ({}) client_id={} offset={}",
                    entry.pid, process_name, entry.client_id, entry.channel_offset
                );
            }
        } else {
            println!(
                "    pid={} ({}) client_id={} offset={}",
                entry.pid, process_name, entry.client_id, entry.channel_offset
            );
        }
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
        CommandRequest::Help => json_error("help is provided by the CLI; run 'prism --help' locally".to_string()),
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
        CommandRequest::Apps => match build_clients_payload(device_id) {
            Ok(payload) => json_success_with_data(payload),
            Err(err) => json_error(format!("failed to fetch apps: {}", err)),
        },
        CommandRequest::SetApp { app_name, offset } => {
            // Find groups by the display name used by the `apps` command
            // (responsible_name if present, otherwise process_name). Match must be exact.
            match build_clients_payload(device_id) {
                Ok(clients) => {
                    // Collect target responsible_pids (groups) and individual pids where responsible_pid is None
                    let mut target_responsible_pids: HashSet<i32> = HashSet::new();
                    let mut direct_pids: Vec<i32> = Vec::new();
                    for client in &clients {
                        let display = client
                            .responsible_name
                            .as_ref()
                            .or(client.process_name.as_ref())
                            .map(|s| s.as_str());
                        if display == Some(app_name.as_str()) {
                            if let Some(rpid) = client.responsible_pid {
                                target_responsible_pids.insert(rpid);
                            } else {
                                direct_pids.push(client.pid);
                            }
                        }
                    }

                    if target_responsible_pids.is_empty() && direct_pids.is_empty() {
                        return json_error(format!("no clients found for app '{}'.", app_name));
                    }

                    let mut results: Vec<RoutingUpdateAck> = Vec::new();
                    let mut errors: Vec<String> = Vec::new();

                    for client in clients {
                        let should_update = if let Some(rpid) = client.responsible_pid {
                            target_responsible_pids.contains(&rpid)
                        } else {
                            direct_pids.contains(&client.pid)
                        };

                        if should_update {
                            match send_rout_update(device_id, client.pid, offset) {
                                Ok(()) => results.push(RoutingUpdateAck { pid: client.pid, channel_offset: offset }),
                                Err(err) => errors.push(format!("failed to set pid {}: {}", client.pid, err)),
                            }
                        }
                    }

                    if results.is_empty() {
                        if errors.is_empty() {
                            return json_error(format!("no clients found for app '{}'.", app_name));
                        } else {
                            return json_error(format!("all matching clients failed for app '{}': {}", app_name, errors.join("; ")));
                        }
                    }

                    if !errors.is_empty() {
                        let msg = format!("partial failures: {}", errors.join("; "));
                        return json_success_with_message_and_data(msg, results);
                    }

                    json_success_with_data(results)
                }
                Err(err) => json_error(format!("failed to fetch clients: {}", err)),
            }
        }
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
        .map(|entry| {
            let process_name = procinfo::process_name(entry.pid);
            let responsible_identity = procinfo::resolve_responsible_identity(entry.pid);
            let (responsible_pid, responsible_name) = if let Some(identity) = responsible_identity {
                let name = identity.preferred_name();
                (Some(identity.pid), name)
            } else {
                (None, None)
            };

            ClientInfoPayload {
                pid: entry.pid,
                client_id: entry.client_id,
                channel_offset: entry.channel_offset,
                process_name,
                responsible_pid,
                responsible_name,
            }
        })
        .collect();

    Ok(payload)
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
