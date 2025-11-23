use coreaudio_sys::*;
use std::ptr;
use std::mem;
use core_foundation::string::CFString;
use core_foundation::base::TCFType;

// Constants
const K_AUDIO_PRISM_PROPERTY_ROUTING_TABLE: AudioObjectPropertySelector = 0x726F7574; // 'rout'

#[repr(C)]
#[allow(non_snake_case)]
struct PrismRoutingUpdate {
    pid: i32,
    channel_offset: u32,
}

fn main() {
    println!("Prism Daemon (prismd) starting...");

    let device_id = match find_prism_device() {
        Some(id) => id,
        Option::None => {
            eprintln!("Prism driver not found!");
            return;
        }
    };

    println!("Found Prism Device ID: {}", device_id);

    // List all properties
    list_properties(device_id);

    let my_pid = std::process::id() as i32;

    // File-based IPC Trigger removed.


    // Test: Send a routing update
    // We'll use our own PID for testing, or a dummy one.
    println!("Sending routing update for PID {} to channel offset 2", my_pid);

    let update_str = format!("PID:{},Offset:2", my_pid);
    let cf_str = CFString::new(&update_str);
    let cf_ref = cf_str.as_concrete_TypeRef();

    let address = AudioObjectPropertyAddress {
        mSelector: K_AUDIO_PRISM_PROPERTY_ROUTING_TABLE, // Use 'rout' as IPC channel
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster,
    };

    let status = unsafe {
        AudioObjectSetPropertyData(
            device_id,
            &address,
            0,
            ptr::null(),
            mem::size_of::<CFStringRef>() as u32,
            &cf_ref as *const _ as *const _,
        )
    };

    if status == 0 {
        println!("Routing update sent successfully!");
    } else {
        eprintln!("Failed to send routing update. Status: {}", status);
    }
}

fn list_properties(device_id: AudioObjectID) {
    println!("Listing properties for device {}...", device_id);
    // We can't easily iterate all properties without knowing them,
    // but we can check if our custom property exists using HasProperty

    let address = AudioObjectPropertyAddress {
        mSelector: K_AUDIO_PRISM_PROPERTY_ROUTING_TABLE,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster, // Try Master
    };

    let has_prop = unsafe {
        AudioObjectHasProperty(device_id, &address)
    };

    println!("Has 'rout' property: {}", has_prop);

    // Check standard property
    let name_address = AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyDeviceName,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster,
    };
    let has_name = unsafe { AudioObjectHasProperty(device_id, &name_address) };
    println!("Has 'lnam' (DeviceName) property: {}", has_name);

    // Print 'cust' value
    println!("'cust' selector value: {}", kAudioObjectPropertyCustomPropertyInfoList);
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
            if uid == "com.petitstrawberry.driver.Prism.Device.V2" {
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
