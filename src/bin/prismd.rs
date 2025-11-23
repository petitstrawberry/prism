#![allow(clippy::missing_safety_doc)]

#[path = "../host.rs"]
mod host;

#[path = "../socket.rs"]
mod socket;

use coreaudio_sys::*;
use host::{
    fetch_client_list, find_prism_device, fourcc_to_string_from_le, read_custom_property_info,
    send_rout_update, ClientEntry, K_AUDIO_PRISM_PROPERTY_CLIENT_LIST,
};
use std::ffi::c_void;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

static CLIENT_LIST: Mutex<Vec<ClientEntry>> = Mutex::new(Vec::new());

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
        println!(
            "    pid={} client_id={} offset={}",
            entry.pid, entry.client_id, entry.channel_offset
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

fn handle_ipc_command(command_line: &str, device_id: AudioObjectID) -> String {
    if command_line.is_empty() {
        return "error: empty command\n".to_string();
    }

    let parts: Vec<&str> = command_line.split_whitespace().collect();
    match parts[0] {
        "help" => ipc_help_message(),
        "set" => {
            if parts.len() < 3 {
                return "error: usage set <PID> <OFFSET>\n".to_string();
            }

            let pid = match parts[1].parse::<i32>() {
                Ok(value) => value,
                Err(_) => return "error: PID must be an integer\n".to_string(),
            };

            let offset = match parts[2].parse::<u32>() {
                Ok(value) => value,
                Err(_) => return "error: OFFSET must be a non-negative integer\n".to_string(),
            };

            match send_rout_update(device_id, pid, offset) {
                Ok(()) => format!("Routing update sent: pid={} offset={}\n", pid, offset),
                Err(err) => format!("error: failed to send routing update: {}\n", err),
            }
        }
        "clients" => match format_clients_response(device_id) {
            Ok(output) => output,
            Err(err) => format!("error: failed to fetch clients: {}\n", err),
        },
        "list" => match format_custom_properties_response(device_id) {
            Ok(output) => output,
            Err(err) => format!("error: failed to read custom properties: {}\n", err),
        },
        "quit" | "exit" => "error: terminating prismd via CLI is not supported\n".to_string(),
        other => format!("error: unknown command '{}'; try 'help'\n", other),
    }
}

fn ipc_help_message() -> String {
    "Commands:\n  set <PID> <OFFSET>  Send routing update\n  list                 Show driver custom properties\n  clients              Show active Prism clients\n".to_string()
}

fn format_clients_response(device_id: AudioObjectID) -> Result<String, String> {
    let clients = fetch_client_list(device_id)?;

    {
        let mut cache = CLIENT_LIST.lock().expect("client list mutex poisoned");
        *cache = clients.clone();
    }

    if clients.is_empty() {
        Ok("No active Prism clients.\n".to_string())
    } else {
        let mut out = String::new();
        out.push_str(&format!("Active Prism clients ({}):\n", clients.len()));
        for entry in clients {
            out.push_str(&format!(
                "    pid={} client_id={} offset={}\n",
                entry.pid, entry.client_id, entry.channel_offset
            ));
        }
        Ok(out)
    }
}

fn format_custom_properties_response(device_id: AudioObjectID) -> Result<String, String> {
    let entries = read_custom_property_info(device_id)?;

    if entries.is_empty() {
        return Ok("No custom properties reported by Prism driver.\n".to_string());
    }

    let mut out = String::new();
    out.push_str(&format!("Custom properties for device {}:\n", device_id));

    for (index, entry) in entries.iter().enumerate() {
        let (selector_text, selector_hex) = format_fourcc(entry.selector);
        let (type_text, type_hex) = format_fourcc(entry.property_data_type);
        out.push_str(&format!(
            "  [{}] selector='{}' (0x{:08X}) type='{}' (0x{:08X}) qualifier=0x{:08X}\n",
            index, selector_text, selector_hex, type_text, type_hex, entry.qualifier_data_type
        ));
    }

    Ok(out)
}

fn format_fourcc(value: u32) -> (String, u32) {
    let text = fourcc_to_string_from_le(value);
    let mut bytes = value.to_le_bytes();
    bytes.reverse();
    (text, u32::from_be_bytes(bytes))
}

fn main() {
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
