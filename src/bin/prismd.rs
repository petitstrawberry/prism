use coreaudio_sys::*;
use core_foundation::base::TCFType;
use core_foundation::data::{CFData, CFDataRef};
use core_foundation::string::CFString;
use plist::Value;
use std::env;
use std::ffi::c_void;
use std::io::{self, BufRead, Cursor, Write};
use std::mem;
use std::ptr;
use std::sync::Mutex;

// Constants
const K_AUDIO_PRISM_PROPERTY_ROUTING_TABLE: AudioObjectPropertySelector = 0x726F7574; // 'rout'
const K_AUDIO_PRISM_PROPERTY_CLIENT_LIST: AudioObjectPropertySelector = 0x636C6E74; // 'clnt'

#[derive(Clone, Debug, Default)]
struct ClientEntry {
    pid: i32,
    client_id: u32,
    channel_offset: u32,
}

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

fn fetch_client_list(device_id: AudioObjectID) -> Result<Vec<ClientEntry>, String> {
    let address = AudioObjectPropertyAddress {
        mSelector: K_AUDIO_PRISM_PROPERTY_CLIENT_LIST,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster,
    };

    let mut data_size = mem::size_of::<CFDataRef>() as u32;
    let mut cfdata_ref: CFDataRef = std::ptr::null();
    let status = unsafe {
        AudioObjectGetPropertyData(
            device_id,
            &address,
            0,
            std::ptr::null(),
            &mut data_size,
            &mut cfdata_ref as *mut _ as *mut _,
        )
    };

    if status != 0 {
        return Err(format!(
            "AudioObjectGetPropertyData('clnt') failed with status {}",
            status
        ));
    }

    if cfdata_ref.is_null() {
        return Ok(Vec::new());
    }

    let cfdata = unsafe { CFData::wrap_under_create_rule(cfdata_ref) };
    let bytes = cfdata.bytes();
    let mut cursor = Cursor::new(bytes);
    let value = Value::from_reader(&mut cursor)
        .map_err(|err| format!("Failed to parse client list plist: {}", err))?;

    Ok(parse_client_list_value(value))
}

fn parse_client_list_value(value: Value) -> Vec<ClientEntry> {
    match value {
        Value::Array(items) => items
            .into_iter()
            .filter_map(|item| match item {
                Value::Dictionary(dict) => {
                    let pid = dict
                        .get("pid")
                        .and_then(|v| v.as_signed_integer())
                        .unwrap_or(0) as i32;
                    let client_id = dict
                        .get("client_id")
                        .and_then(|v| v.as_unsigned_integer())
                        .unwrap_or(0) as u32;
                    let channel_offset = dict
                        .get("channel_offset")
                        .and_then(|v| v.as_unsigned_integer())
                        .unwrap_or(0) as u32;
                    Some(ClientEntry {
                        pid,
                        client_id,
                        channel_offset,
                    })
                }
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn print_client_snapshot() {
    let cache = CLIENT_LIST.lock().expect("client list mutex poisoned");
    if cache.is_empty() {
        println!("No active clients.");
        return;
    }

    println!("Current clients ({}):", cache.len());
    for entry in cache.iter() {
        println!(
            "    pid={} client_id={} offset={}",
            entry.pid, entry.client_id, entry.channel_offset
        );
    }
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

#[repr(C)]
#[derive(Debug, Clone, Copy)]
#[allow(non_snake_case)]
struct PrismRoutingUpdate {
    pid: i32,
    channel_offset: u32,
}

fn main() {
    println!("Prism Daemon (prismd) starting...");

    let device_id = if let Some(id) = find_prism_device() {
        id
    } else {
        eprintln!("Prism driver not found!");
        return;
    };

    println!("Found Prism Device ID: {}", device_id);

    match register_client_list_listener(device_id) {
        Ok(()) => {
            if let Err(err) = handle_client_list_update(device_id) {
                eprintln!("[prismd] Initial client list fetch failed: {}", err);
            }
        }
        Err(err) => eprintln!("[prismd] Failed to register client list listener: {}", err),
    }

    // If invoked with args, run a single command and exit.
    // Usage: prismd set <PID> <OFFSET>
    let args: Vec<String> = env::args().collect();
    if args.len() >= 2 {
        if args[1] == "set" && args.len() >= 4 {
            if let (Ok(pid), Ok(offset)) = (args[2].parse::<i32>(), args[3].parse::<u32>()) {
                println!("[prismd] CLI set: pid={} offset={}", pid, offset);
                // Sanity checks before calling into CoreAudio
                let address = AudioObjectPropertyAddress {
                    mSelector: K_AUDIO_PRISM_PROPERTY_ROUTING_TABLE,
                    mScope: kAudioObjectPropertyScopeGlobal,
                    mElement: kAudioObjectPropertyElementMaster,
                };
                let has = unsafe { AudioObjectHasProperty(device_id, &address) };
                println!("[prismd] HasProperty('rout') = {}", has);
                // AudioObjectIsPropertySettable expects a pointer to Boolean (UInt8); use u8 to match
                let mut settable_u8: u8 = 0;
                let is_settable_err = unsafe { AudioObjectIsPropertySettable(device_id, &address, &mut settable_u8 as *mut u8 as *mut _) };
                println!("[prismd] AudioObjectIsPropertySettable returned err={}, is_settable={}", is_settable_err, settable_u8);

                match send_rout_update(device_id, pid, offset) {
                    Ok(()) => println!("Routing update sent: PID={}, Offset={}", pid, offset),
                    Err(s) => eprintln!("Failed to send routing update: {}", s),
                }
            } else {
                eprintln!("Invalid arguments. Usage: prismd set <PID> <OFFSET>");
            }
            return;
        }
    }

    // Interactive REPL for ad-hoc routing updates
    println!("Entering interactive mode. Type 'help' for commands.");
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let _ = stdout.write_all(b"> ");
    let _ = stdout.flush();
    for line in stdin.lock().lines() {
        let line = match line { Ok(l) => l.trim().to_string(), Err(_) => continue };
        if line.is_empty() { continue; }
        let parts: Vec<&str> = line.split_whitespace().collect();
        match parts[0] {
            "help" => {
                println!("Commands:\n  set <PID> <OFFSET>  - send routing update\n  list                - list device properties\n  clients             - show current clients\n  exit                - quit");
            }
            "set" => {
                if parts.len() >= 3 {
                    if let (Ok(pid), Ok(offset)) = (parts[1].parse::<i32>(), parts[2].parse::<u32>()) {
                        match send_rout_update(device_id, pid, offset) {
                            Ok(()) => println!("Routing update sent: PID={}, Offset={}", pid, offset),
                            Err(s) => eprintln!("Failed to send routing update: {}", s),
                        }
                    } else { eprintln!("Invalid PID or OFFSET"); }
                } else { eprintln!("Usage: set <PID> <OFFSET>"); }
            }
            "list" => { list_properties(device_id); }
            "clients" => { print_client_snapshot(); }
            "exit" | "quit" => { break; }
            _ => { eprintln!("Unknown command: {}", parts[0]); }
        }
        let _ = stdout.write_all(b"> ");
        let _ = stdout.flush();
    }
}

fn send_rout_update(device_id: AudioObjectID, pid: i32, offset: u32) -> Result<(), String> {
    let update = PrismRoutingUpdate { pid, channel_offset: offset };
    let address = AudioObjectPropertyAddress {
        mSelector: K_AUDIO_PRISM_PROPERTY_ROUTING_TABLE,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster,
    };

    // Debug print before calling into CoreAudio
    println!("[prismd] send_rout_update: device={}, pid={}, offset={}, size={}",
        device_id, update.pid, update.channel_offset, mem::size_of::<PrismRoutingUpdate>());

    // Wrap the binary struct into a CFData (CFPropertyList) so it matches
    // the driver's advertised mPropertyDataType = 'plst'. Drivers that expect
    // a plist will treat the CFData as a CFPropertyList containing the bytes.
    let mut buf: Vec<u8> = Vec::with_capacity(mem::size_of::<PrismRoutingUpdate>());
    buf.extend_from_slice(&update.pid.to_le_bytes());
    buf.extend_from_slice(&update.channel_offset.to_le_bytes());

    let cfdata = CFData::from_buffer(&buf);
    let cfdata_ref = cfdata.as_concrete_TypeRef();
    // Pass a pointer to the CFDataRef and size = sizeof(CFDataRef)
    let status = unsafe {
        AudioObjectSetPropertyData(
            device_id,
            &address,
            0,
            ptr::null(),
            mem::size_of::<CFDataRef>() as u32,
            &cfdata_ref as *const _ as *const _,
        )
    };

    println!("[prismd] AudioObjectSetPropertyData returned status={}", status);

    if status == 0 { Ok(()) } else {
        Err(format!("OSStatus {}", status))
    }
}

fn list_properties(device_id: AudioObjectID) {
    println!("Listing custom properties (cust) for device {}...", device_id);

    // Define the address for the custom property info list ('cust')
    let cust_address = AudioObjectPropertyAddress {
        mSelector: kAudioObjectPropertyCustomPropertyInfoList,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster,
    };

    // Get size
    let mut data_size: u32 = 0;
    let status_size = unsafe { AudioObjectGetPropertyDataSize(device_id, &cust_address, 0, std::ptr::null(), &mut data_size) };
    if status_size != 0 || data_size == 0 {
        println!("No custom properties or failed to get size (status={} size={})", status_size, data_size);
        return;
    }

    // Read data
    let mut buffer = vec![0u8; data_size as usize];
    let mut read_size = data_size;
    let status = unsafe { AudioObjectGetPropertyData(device_id, &cust_address, 0, std::ptr::null(), &mut read_size, buffer.as_mut_ptr() as *mut _) };
    if status != 0 {
        println!("Failed to read 'cust' data: status={}", status);
        return;
    }

    // Parse entries as AudioServerPlugInCustomPropertyInfo structs (3 * u32)
    #[repr(C)]
    #[derive(Debug, Clone, Copy)]
    #[allow(non_snake_case)]
    struct AudioServerPlugInCustomPropertyInfo {
        mSelector: u32,
        mPropertyDataType: u32,
        mQualifierDataType: u32,
    }

    #[allow(dead_code)]
    fn fourcc_to_string(v: u32) -> String {
        let bytes = v.to_be_bytes();
        let s = std::str::from_utf8(&bytes).unwrap_or("????");
        s.to_string()
    }

    let entry_size = mem::size_of::<AudioServerPlugInCustomPropertyInfo>();
    println!("cust data size: {} bytes, entry size: {}", read_size, entry_size);
    if read_size as usize % entry_size != 0 {
        println!("Unexpected cust data size");
    }

    for (i, chunk) in buffer.chunks(entry_size).enumerate() {
        // SAFETY: chunk is exactly entry_size bytes
        let mut sel_bytes = [0u8; 4];
        sel_bytes.copy_from_slice(&chunk[0..4]);
        let mut dtype_bytes = [0u8; 4];
        dtype_bytes.copy_from_slice(&chunk[4..8]);
        let mut qual_bytes = [0u8; 4];
        qual_bytes.copy_from_slice(&chunk[8..12]);

        // The HAL uses native little-endian storage on x86_64. The bytes in the buffer
        // appear in little-endian order (LSB first), so to recover the ASCII selector
        // string we need to reverse the byte order. Convert the fields using little-endian
        // interpretation for numeric values and reverse bytes for human-readable fourcc.
        let info = AudioServerPlugInCustomPropertyInfo {
            mSelector: u32::from_le_bytes(sel_bytes),
            mPropertyDataType: u32::from_le_bytes(dtype_bytes),
            mQualifierDataType: u32::from_le_bytes(qual_bytes),
        };

        let mut sel_be = sel_bytes; sel_be.reverse();
        let mut dtype_be = dtype_bytes; dtype_be.reverse();

        let sel_str = std::str::from_utf8(&sel_be).unwrap_or("????");
        let dtype_str = std::str::from_utf8(&dtype_be).unwrap_or("????");
        let sel_hex = u32::from_be_bytes(sel_be);
        let dtype_hex = u32::from_be_bytes(dtype_be);

        println!("  [{}] Selector: '{}' (0x{:08X}), Type: '{}' (0x{:08X}), Qualifier: 0x{:08X}",
            i,
            sel_str,
            sel_hex,
            dtype_str,
            dtype_hex,
            info.mQualifierDataType,
        );
    }
}

fn find_prism_device() -> Option<AudioObjectID> {
    let address = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyDevices,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster,
    };

    let mut data_size: u32 = 0;
    let status = unsafe {
        AudioObjectGetPropertyDataSize(
            kAudioObjectSystemObject,
            &address,
            0,
            ptr::null(),
            &mut data_size,
        )
    };

    if status != 0 {
        eprintln!("Error getting device list size: {}", status);
        return None;
    }

    let device_count = data_size / mem::size_of::<AudioObjectID>() as u32;
    let mut device_ids: Vec<AudioObjectID> = vec![0; device_count as usize];

    let status = unsafe {
        AudioObjectGetPropertyData(
            kAudioObjectSystemObject,
            &address,
            0,
            ptr::null(),
            &mut data_size,
            device_ids.as_mut_ptr() as *mut _,
        )
    };

    if status != 0 {
        eprintln!("Error getting device list: {}", status);
        return None;
    }

    for device_id in device_ids {
        if let Some(uid) = get_device_uid(device_id) {
            // Check for Prism UID
            if uid == "com.petitstrawberry.driver.Prism.Device" {
                return Some(device_id);
            }
        }
    }

    None
}

fn get_device_uid(device_id: AudioObjectID) -> Option<String> {
    let address = AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyDeviceUID,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster,
    };

    let mut data_size = mem::size_of::<CFStringRef>() as u32;
    let mut uid_ref: *const std::ffi::c_void = ptr::null();

    let status = unsafe {
        AudioObjectGetPropertyData(
            device_id,
            &address,
            0,
            ptr::null(),
            &mut data_size,
            &mut uid_ref as *mut _ as *mut _,
        )
    };

    if status != 0 || uid_ref.is_null() {
        return None;
    }

    unsafe {
        // Wrap in CFString to manage lifetime and conversion
        // We use wrap_under_create_rule because GetPropertyData returns a reference we own (Copy Rule applies to GetPropertyData?)
        // Wait, AudioObjectGetPropertyData follows the "Get" rule.
        // "The caller is responsible for releasing the returned object." -> This is the Create Rule.
        // So wrap_under_create_rule is correct.
        let cf_string = CFString::wrap_under_create_rule(uid_ref as core_foundation::string::CFStringRef);
        Some(cf_string.to_string())
    }
}
