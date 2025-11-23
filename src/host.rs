use core_foundation::base::TCFType;
use core_foundation::data::{CFData, CFDataRef};
use core_foundation::string::{CFString, CFStringRef};
use coreaudio_sys::*;
use plist::Value;
use std::ffi::c_void;
use std::io::Cursor;
use std::mem;
use std::ptr;

#[allow(dead_code)]
pub const K_AUDIO_PRISM_PROPERTY_ROUTING_TABLE: AudioObjectPropertySelector = 0x726F7574; // 'rout'
pub const K_AUDIO_PRISM_PROPERTY_CLIENT_LIST: AudioObjectPropertySelector = 0x636C6E74; // 'clnt'

#[derive(Clone, Debug, Default)]
pub struct ClientEntry {
    pub pid: i32,
    pub client_id: u32,
    pub channel_offset: u32,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Default)]
pub struct CustomPropertyInfo {
    pub selector: u32,
    pub property_data_type: u32,
    pub qualifier_data_type: u32,
}

#[allow(dead_code)]
pub fn send_rout_update(device_id: AudioObjectID, pid: i32, offset: u32) -> Result<(), String> {
    let update = PrismRoutingUpdate {
        pid,
        channel_offset: offset,
    };

    let address = AudioObjectPropertyAddress {
        mSelector: K_AUDIO_PRISM_PROPERTY_ROUTING_TABLE,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster,
    };

    let mut buf: Vec<u8> = Vec::with_capacity(mem::size_of::<PrismRoutingUpdate>());
    buf.extend_from_slice(&update.pid.to_le_bytes());
    buf.extend_from_slice(&update.channel_offset.to_le_bytes());

    let cfdata = CFData::from_buffer(&buf);
    let cfdata_ref = cfdata.as_concrete_TypeRef();
    let status = unsafe {
        AudioObjectSetPropertyData(
            device_id,
            &address,
            0,
            ptr::null(),
            mem::size_of::<CFDataRef>() as u32,
            &cfdata_ref as *const _ as *const c_void,
        )
    };

    if status == 0 {
        Ok(())
    } else {
        Err(format!(
            "AudioObjectSetPropertyData failed with status {}",
            status
        ))
    }
}

pub fn fetch_client_list(device_id: AudioObjectID) -> Result<Vec<ClientEntry>, String> {
    let address = AudioObjectPropertyAddress {
        mSelector: K_AUDIO_PRISM_PROPERTY_CLIENT_LIST,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster,
    };

    let mut data_size = mem::size_of::<CFDataRef>() as u32;
    let mut cfdata_ref: CFDataRef = ptr::null();
    let status = unsafe {
        AudioObjectGetPropertyData(
            device_id,
            &address,
            0,
            ptr::null(),
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

pub fn parse_client_list_value(value: Value) -> Vec<ClientEntry> {
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

#[allow(dead_code)]
pub fn read_custom_property_info(
    device_id: AudioObjectID,
) -> Result<Vec<CustomPropertyInfo>, String> {
    let cust_address = AudioObjectPropertyAddress {
        mSelector: kAudioObjectPropertyCustomPropertyInfoList,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster,
    };

    let mut data_size: u32 = 0;
    let status_size = unsafe {
        AudioObjectGetPropertyDataSize(device_id, &cust_address, 0, ptr::null(), &mut data_size)
    };

    if status_size != 0 {
        return Err(format!(
            "AudioObjectGetPropertyDataSize('cust') failed with status {}",
            status_size
        ));
    }

    if data_size == 0 {
        return Ok(Vec::new());
    }

    let mut buffer = vec![0u8; data_size as usize];
    let mut read_size = data_size;
    let status = unsafe {
        AudioObjectGetPropertyData(
            device_id,
            &cust_address,
            0,
            ptr::null(),
            &mut read_size,
            buffer.as_mut_ptr() as *mut _,
        )
    };

    if status != 0 {
        return Err(format!(
            "AudioObjectGetPropertyData('cust') failed with status {}",
            status
        ));
    }

    if read_size == 0 {
        return Ok(Vec::new());
    }

    #[allow(non_snake_case)]
    #[repr(C)]
    #[derive(Debug, Clone, Copy)]
    struct AudioServerPlugInCustomPropertyInfoRaw {
        mSelector: u32,
        mPropertyDataType: u32,
        mQualifierDataType: u32,
    }

    let entry_size = mem::size_of::<AudioServerPlugInCustomPropertyInfoRaw>();
    if !(read_size as usize).is_multiple_of(entry_size) {
        return Err("Unexpected 'cust' data size".to_string());
    }

    let mut out = Vec::new();
    for chunk in buffer.chunks(entry_size) {
        let raw = unsafe { *(chunk.as_ptr() as *const AudioServerPlugInCustomPropertyInfoRaw) };

        out.push(CustomPropertyInfo {
            selector: raw.mSelector,
            property_data_type: raw.mPropertyDataType,
            qualifier_data_type: raw.mQualifierDataType,
        });
    }

    Ok(out)
}

#[allow(dead_code)]
pub fn fourcc_to_string_from_le(value: u32) -> String {
    let mut bytes = value.to_le_bytes();
    bytes.reverse();
    std::str::from_utf8(&bytes).unwrap_or("????").to_string()
}

pub fn find_prism_device() -> Result<AudioObjectID, String> {
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
        return Err(format!("Error getting device list size: {}", status));
    }

    let device_count = data_size / mem::size_of::<AudioObjectID>() as u32;
    if device_count == 0 {
        return Err("No audio devices found".to_string());
    }

    let mut device_ids: Vec<AudioObjectID> = vec![0; device_count as usize];
    let mut list_size = data_size;
    let status = unsafe {
        AudioObjectGetPropertyData(
            kAudioObjectSystemObject,
            &address,
            0,
            ptr::null(),
            &mut list_size,
            device_ids.as_mut_ptr() as *mut _,
        )
    };

    if status != 0 {
        return Err(format!("Error getting device list: {}", status));
    }

    for device_id in device_ids {
        if let Some(uid) = get_device_uid(device_id) {
            if uid == "dev.ichigo.driver.Prism.Device" {
                return Ok(device_id);
            }
        }
    }

    Err("Prism device not found".to_string())
}

fn get_device_uid(device_id: AudioObjectID) -> Option<String> {
    let address = AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyDeviceUID,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster,
    };

    let mut data_size = mem::size_of::<CFStringRef>() as u32;
    let mut uid_ref: CFStringRef = ptr::null();

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
        let cf_string = CFString::wrap_under_create_rule(uid_ref);
        Some(cf_string.to_string())
    }
}

#[allow(dead_code)]
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct PrismRoutingUpdate {
    pid: i32,
    channel_offset: u32,
}
