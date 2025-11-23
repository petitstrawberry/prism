use coreaudio_sys::*;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, AtomicI32, AtomicBool, Ordering};
use std::io::BufRead;
// use std::collections::HashMap; // Removed
// use std::sync::RwLock; // Removed

#[derive(Debug, Clone, Copy)]
pub struct PrismConfig {
    pub buffer_frame_size: u32,
    pub safety_offset: u32,
    pub ring_buffer_frame_size: u32,
    pub zero_timestamp_period: u32,
    pub num_channels: u32,
}

impl PrismConfig {
    fn default() -> Self {
        Self {
            buffer_frame_size: 1024,
            safety_offset: 256,
            ring_buffer_frame_size: 1024,
            zero_timestamp_period: 1024,
            num_channels: 64, // Increased to 64 for OMNIBUS-style routing
        }
    }

    fn load() -> Self {
        let mut config = Self::default();
        let path = "/Library/Application Support/Prism/config.txt";

        if let Ok(file) = std::fs::File::open(path) {
            let reader = std::io::BufReader::new(file);
            for line in reader.lines() {
                if let Ok(l) = line {
                    // Ignore comments
                    if l.trim().starts_with('#') { continue; }
                    let parts: Vec<&str> = l.split('=').collect();
                    if parts.len() == 2 {
                        let key = parts[0].trim();
                        if let Ok(val) = parts[1].trim().parse::<u32>() {
                            match key {
                                "buffer_frame_size" => config.buffer_frame_size = val,
                                "safety_offset" => config.safety_offset = val,
                                "ring_buffer_frame_size" => config.ring_buffer_frame_size = val,
                                "zero_timestamp_period" => config.zero_timestamp_period = val,
                                "num_channels" => config.num_channels = val,
                                _ => {}
                            }
                        }
                    }
                }
            }
            log_msg(&format!("Prism: Loaded config from {}", path));
            return config;
        }
        log_msg("Prism: Using default config");
        config
    }
}

// Define the Host Interface struct locally since coreaudio-sys seems to treat it as opaque or we are having trouble dereferencing it.
// This layout must match the C definition of AudioServerPlugInHostInterface.
// Removed PrismHostInterface as it is not used yet.

// UUID for the driver interface (kAudioServerPlugInDriverInterfaceUUID)
// This should match what is expected by Core Audio.
// In coreaudio-sys, this might be available as a constant, but often we need to construct it.
// For now, we'll use the standard UUID for the driver interface.

const MAX_CLIENTS: usize = 4096; // Increased for Direct Indexing

pub struct ClientSlot {
    pub client_id: AtomicU32,
    pub channel_offset: AtomicUsize,
    pub pid: AtomicI32,
}

#[repr(C)]
pub struct PrismDriver {
    pub _vtable: *const AudioServerPlugInDriverInterface,
    pub ref_count: AtomicU32,
    pub host: Option<AudioServerPlugInHostRef>,
    pub anchor_host_time: AtomicU64,
    pub num_time_stamps: AtomicU64,
    pub host_ticks_per_frame: f64,
    pub client_count: AtomicU32,
    pub phase: f64,
    pub loopback_buffer: Vec<f32>,
    pub config: PrismConfig,

    // Padding to prevent false sharing between write_pos and read_pos
    // Cache line size is typically 64 bytes.
    pub _pad1: [u8; 64],
    pub write_pos: AtomicUsize,
    pub _pad2: [u8; 64],
    pub read_pos: AtomicUsize,

    // Fixed size array of client slots for lock-free access in IO path
    pub client_slots: Vec<ClientSlot>,
}// The singleton instance of our driver
static mut DRIVER_INSTANCE: *mut PrismDriver = ptr::null_mut();

#[allow(deprecated)]
fn get_host_ticks_per_second() -> f64 {
    let mut info = libc::mach_timebase_info_data_t { numer: 0, denom: 0 };
    unsafe {
        libc::mach_timebase_info(&mut info);
    }
    if info.numer == 0 || info.denom == 0 {
        return 1_000_000_000.0;
    }
    // ticks * numer / denom = nanoseconds
    // 1 sec = 1e9 ns
    // ticks_per_sec * numer / denom = 1e9
    // ticks_per_sec = 1e9 * denom / numer
    1_000_000_000.0 * (info.denom as f64) / (info.numer as f64)
}

// --- IUnknown Implementation ---

unsafe extern "C" fn query_interface(
    _self: *mut c_void,
    _uuid: CFUUIDBytes,
    _out_interface: *mut *mut c_void,
) -> HRESULT {
    // Minimal implementation: We only support IUnknown and the Driver Interface.
    // For now, just return S_OK and self, assuming the caller asks for the right thing.
    // TODO: Proper UUID check.
    log_msg(&format!("Prism: QueryInterface called. UUID: {:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        _uuid.byte0, _uuid.byte1, _uuid.byte2, _uuid.byte3,
        _uuid.byte4, _uuid.byte5,
        _uuid.byte6, _uuid.byte7,
        _uuid.byte8, _uuid.byte9,
        _uuid.byte10, _uuid.byte11, _uuid.byte12, _uuid.byte13, _uuid.byte14, _uuid.byte15
    ));
    *_out_interface = _self;
    add_ref(_self);
    0 // S_OK
}

unsafe extern "C" fn add_ref(_self: *mut c_void) -> ULONG {
    let driver = _self as *mut PrismDriver;
    (*driver).ref_count.fetch_add(1, Ordering::Relaxed) + 1
}

unsafe extern "C" fn release(_self: *mut c_void) -> ULONG {
    let driver = _self as *mut PrismDriver;
    let count = (*driver).ref_count.fetch_sub(1, Ordering::Relaxed) - 1;
    if count == 0 {
        // In a real scenario, we might drop the Box here.
        // But for a driver that lives as long as the server, we might keep it.
    }
    count
}

// --- Driver Interface Implementation (Stubs) ---

unsafe extern "C" fn initialize(
    _self: AudioServerPlugInDriverRef,
    host: AudioServerPlugInHostRef,
) -> OSStatus {
    log_msg(&format!(
        "Prism: Initialize called!!! - ver {} (cust_any=true, rout_any=true)",
        env!("CARGO_PKG_VERSION")
    ));
    let driver = _self as *mut PrismDriver;
    (*driver).host = Some(host);

    if let Some(prop_changed) = (*host).PropertiesChanged {
        // 1. Device List (プラグインレベル)
        let addr_dev_list = AudioObjectPropertyAddress {
            mSelector: kAudioPlugInPropertyDeviceList,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMaster,
        };
        prop_changed(host, kAudioObjectPlugInObject, 1, &addr_dev_list);

        // 2. Custom Property Info (カタログ更新)
        let addr_cust = AudioObjectPropertyAddress {
            mSelector: kAudioObjectPropertyCustomPropertyInfoList,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMaster,
        };
        prop_changed(host, kAudioObjectPlugInObject, 1, &addr_cust);
        prop_changed(host, DEVICE_ID, 1, &addr_cust);

        // ★ 3. Device Name
        let addr_name = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyDeviceName,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMaster,
        };
        prop_changed(host, DEVICE_ID, 1, &addr_name);

        // 4. Routing Table (routも念のため)
        let addr_rout = AudioObjectPropertyAddress {
            mSelector: kAudioPrismPropertyRoutingTable,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMaster,
        };
        prop_changed(host, DEVICE_ID, 1, &addr_rout);

        let addr_owned = AudioObjectPropertyAddress {
            mSelector: kAudioObjectPropertyOwnedObjects,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMaster,
        };
        prop_changed(host, DEVICE_ID, 1, &addr_owned);

        let addr_streams = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyStreams,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMaster,
        };
        prop_changed(host, DEVICE_ID, 1, &addr_streams);
    }

    0
}

unsafe extern "C" fn create_device(
    _self: AudioServerPlugInDriverRef,
    _description: CFDictionaryRef,
    _client_id: *const AudioServerPlugInClientInfo,
    _out_device_id: *mut AudioObjectID,
) -> OSStatus {
    kAudioHardwareUnsupportedOperationError as OSStatus
}

unsafe extern "C" fn destroy_device(
    _self: AudioServerPlugInDriverRef,
    _device_id: AudioObjectID,
) -> OSStatus {
    kAudioHardwareUnsupportedOperationError as OSStatus
}

unsafe extern "C" fn add_device_client(
    _self: AudioServerPlugInDriverRef,
    _device_id: AudioObjectID,
    _client_id: *const AudioServerPlugInClientInfo,
) -> OSStatus {
    let driver = _self as *mut PrismDriver;
    if !_client_id.is_null() {
        // Cast to our custom struct to access mBundleID
        let client_info = &*(_client_id as *const PrismClientInfo);
        let client_id = client_info.mClientID;
        let pid = client_info.mProcessID;

        // Direct Indexing for slot
        let idx = (client_id as usize) & (MAX_CLIENTS - 1);
        let slots = &(*driver).client_slots;
        let slot = &slots[idx];

        // Prism 2.0: Dumb Driver
        // We default to channel 0 (Passthrough) or a specific "unassigned" state.
        // The Daemon will update this via SetProperty('rout').
        let channel_offset = 0;

        log_msg(&format!("Prism: Client Added. ID={}, PID={}, Slot={}, Default Offset={}", client_id, pid, idx, channel_offset));

        slot.channel_offset.store(channel_offset, Ordering::SeqCst);
        slot.pid.store(pid as i32, Ordering::SeqCst);
        slot.client_id.store(client_id, Ordering::Release);
    }
    0
}

unsafe extern "C" fn remove_device_client(
    _self: AudioServerPlugInDriverRef,
    _device_id: AudioObjectID,
    _client_id: *const AudioServerPlugInClientInfo,
) -> OSStatus {
    let driver = _self as *mut PrismDriver;
    if !_client_id.is_null() {
        let client_info = &*_client_id;
        let client_id = client_info.mClientID;
        let pid = client_info.mProcessID;

        log_msg(&format!("Prism: Client Removed. ID={}, PID={}", client_id, pid));

        let idx = (client_id as usize) & (MAX_CLIENTS - 1);
        let slots = &(*driver).client_slots;
        let slot = &slots[idx];
        let id = slot.client_id.load(Ordering::SeqCst);

        if id == client_id {
            slot.client_id.store(0, Ordering::Release); // Reset to 0
            slot.channel_offset.store(0, Ordering::Relaxed);
            slot.pid.store(0, Ordering::Relaxed);
        }
    }
    0
}

unsafe extern "C" fn perform_device_configuration_change(
    _self: AudioServerPlugInDriverRef,
    _device_id: AudioObjectID,
    _action: u64,
    _change_info: *mut c_void,
) -> OSStatus {
    0
}

unsafe extern "C" fn abort_device_configuration_change(
    _self: AudioServerPlugInDriverRef,
    _device_id: AudioObjectID,
    _action: u64,
    _change_info: *mut c_void,
) -> OSStatus {
    0
}

// Constants
const DEVICE_ID: AudioObjectID = 2;
const INPUT_STREAM_ID: AudioObjectID = 3;
const OUTPUT_STREAM_ID: AudioObjectID = 4;

#[allow(non_upper_case_globals)]
const kAudioPlugInPropertyDeviceList: AudioObjectPropertySelector = 0x64657623; // 'dev#'
#[allow(non_upper_case_globals)]
const kAudioPlugInPropertyResourceBundle: AudioObjectPropertySelector = 0x72737263; // 'rsrc'
#[allow(non_upper_case_globals)]
const kAudioPlugInPropertyTranslateUIDToDevice: AudioObjectPropertySelector = 0x75696464; // 'uidd'
#[allow(non_upper_case_globals)]
const kAudioObjectPropertyScope: AudioObjectPropertySelector = 0x73636F70; // 'scop'
#[allow(non_upper_case_globals)]
const kAudioObjectPropertyElement: AudioObjectPropertySelector = 0x656C656D; // 'elem'
#[allow(non_upper_case_globals)]
const kAudioDevicePropertyBufferFrameSize: AudioObjectPropertySelector = 0x6673697A; // 'fsiz'
#[allow(non_upper_case_globals)]
const kAudioDevicePropertyBufferFrameSizeRange: AudioObjectPropertySelector = 0x66737A72; // 'fszr'
#[allow(non_upper_case_globals)]
const kAudioObjectPropertyControlList: AudioObjectPropertySelector = 0x6374726C; // 'ctrl'
#[allow(non_upper_case_globals)]
const kAudioObjectPropertyCustomPropertyInfoList: AudioObjectPropertySelector = 0x63757374; // 'cust'
#[allow(non_upper_case_globals)]
const kAudioDevicePropertyStreamsIsSettable: AudioObjectPropertySelector = 0x7369736F; // 'siso'
#[allow(non_upper_case_globals)]
const kAudioDevicePropertyClockDomain: AudioObjectPropertySelector = 0x636C6B64; // 'clkd'
#[allow(non_upper_case_globals)]
const kAudioDevicePropertyClockSource: AudioObjectPropertySelector = 0x63737263; // 'csrc'
#[allow(non_upper_case_globals)]
const kAudioDevicePropertyIsHidden: AudioObjectPropertySelector = 0x6869646E; // 'hidn'
#[allow(non_upper_case_globals)]
const kAudioObjectPropertyName: AudioObjectPropertySelector = 0x6C6E616D; // 'lnam'
#[allow(non_upper_case_globals)]
const kAudioDevicePropertyRingBufferFrameSize: AudioObjectPropertySelector = 0x72696E67; // 'ring'
#[allow(non_upper_case_globals)]
const kAudioPrismPropertyRoutingTable: AudioObjectPropertySelector = 0x726F7574; // 'rout'
#[allow(non_upper_case_globals)]
const kAudioObjectPropertySelectorNone: AudioObjectPropertySelector = 0;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
#[allow(non_snake_case)]
struct PrismRoutingUpdate {
    pid: i32,
    channel_offset: u32,
}

#[repr(C)]
#[allow(non_snake_case)]
struct AudioServerPlugInCustomPropertyInfo {
    mSelector: AudioObjectPropertySelector,
    mPropertyDataType: AudioObjectPropertySelector,
    mQualifierDataType: AudioObjectPropertySelector,
}

#[allow(non_upper_case_globals)]
unsafe extern "C" fn has_property(
    _self: AudioServerPlugInDriverRef,
    object_id: AudioObjectID,
    _client_process_id: pid_t,
    _address: *const AudioObjectPropertyAddress,
) -> Boolean {
    let address = *_address;
    let selector = address.mSelector;

    // ★ 削除: ここにあった Global な forced true は消す！
    // 厳密に match の中で判定します。

    let res = match object_id {
        // --------------------------------------------------------
        // 1. Plugin Object
        // --------------------------------------------------------
        id if id == kAudioObjectPlugInObject => {
            if selector == kAudioObjectPropertyBaseClass ||
               selector == kAudioObjectPropertyClass ||
               selector == kAudioObjectPropertyOwner ||
               selector == kAudioObjectPropertyManufacturer ||
               selector == kAudioObjectPropertyOwnedObjects ||
               selector == kAudioPlugInPropertyDeviceList ||
               selector == kAudioPlugInPropertyTranslateUIDToDevice ||
               selector == kAudioPlugInPropertyResourceBundle ||
               selector == kAudioObjectPropertyCustomPropertyInfoList {
                log_msg(&format!("Prism: HasProperty Plugin Known. Object: {}, Selector: {}", object_id, selector));
                true
            } else {
                log_msg(&format!("Prism: HasProperty Plugin Unknown. Object: {}, Selector: {}", object_id, selector));
                false
            }
        },

        // --------------------------------------------------------
        // 2. Device Object (ここだけ rout / cust を許可)
        // --------------------------------------------------------
        DEVICE_ID => {
            if selector == kAudioObjectPropertyBaseClass ||
               selector == kAudioObjectPropertyClass ||
               selector == kAudioObjectPropertyOwner ||
               selector == kAudioObjectPropertyManufacturer ||
               selector == kAudioObjectPropertyOwnedObjects ||
               selector == kAudioObjectPropertyControlList ||
               selector == kAudioObjectPropertyCustomPropertyInfoList || // 'cust' OK
               selector == kAudioDevicePropertyStreams ||
               selector == kAudioDevicePropertyStreamsIsSettable ||
               selector == kAudioDevicePropertyDeviceUID ||
               selector == kAudioDevicePropertyModelUID ||
               selector == kAudioDevicePropertyDeviceName ||
               selector == kAudioObjectPropertyName ||
               selector == kAudioDevicePropertyDeviceIsRunning ||
               selector == kAudioDevicePropertyIsHidden ||
               selector == kAudioDevicePropertyDeviceCanBeDefaultDevice ||
               selector == kAudioDevicePropertyDeviceCanBeDefaultSystemDevice ||
               selector == kAudioDevicePropertySafetyOffset ||
               selector == kAudioDevicePropertyLatency ||
               selector == kAudioDevicePropertyDeviceIsAlive ||
               selector == kAudioDevicePropertyNominalSampleRate ||
               selector == kAudioDevicePropertyAvailableNominalSampleRates ||
               selector == kAudioDevicePropertyBufferFrameSize ||
               selector == kAudioDevicePropertyBufferFrameSizeRange ||
               selector == kAudioDevicePropertyRingBufferFrameSize ||
               selector == kAudioDevicePropertyZeroTimeStampPeriod ||
               selector == kAudioDevicePropertyClockDomain ||
               selector == kAudioDevicePropertyClockSource ||
               selector == kAudioDevicePropertyDataSource ||
               selector == kAudioObjectPropertyScope ||
               selector == kAudioObjectPropertyElement ||
               selector == kAudioDevicePropertyBufferFrameSize ||
               selector == kAudioPrismPropertyRoutingTable { // 'rout' OK
                log_msg(&format!("Prism: HasProperty Device Known. Object: {}, Selector: {}", object_id, selector));
                true
            } else {
                log_msg(&format!("Prism: HasProperty Device Unknown. Object: {}, Selector: {}", object_id, selector));
                false
            }
        },

        // --------------------------------------------------------
        // 3. Stream Object (cust / rout は削除！)
        // --------------------------------------------------------
        INPUT_STREAM_ID | OUTPUT_STREAM_ID => {
            if selector == kAudioObjectPropertyBaseClass ||
               selector == kAudioObjectPropertyClass ||
               selector == kAudioObjectPropertyOwner ||
               selector == kAudioObjectPropertyControlList ||
               // ★ 削除: kAudioObjectPropertyCustomPropertyInfoList はここに入れない
               selector == kAudioStreamPropertyDirection ||
               selector == kAudioStreamPropertyTerminalType ||
               selector == kAudioStreamPropertyStartingChannel ||
               selector == kAudioObjectPropertyScope ||
               selector == kAudioObjectPropertyElement {
                log_msg(&format!("Prism: HasProperty Stream Known. Object: {}, Selector: {}", object_id, selector));
                true
            } else {
                log_msg(&format!("Prism: HasProperty Stream Unknown. Object: {}, Selector: {}", object_id, selector));
                false
            }
        },
        _ => {
            log_msg(&format!("Prism: HasProperty Unknown. Object: {}, Selector: {}", object_id, selector));
            false
        }
    };

    if res { 1 } else { 0 }
}

#[allow(non_upper_case_globals)]
unsafe extern "C" fn is_property_settable(
    _self: AudioServerPlugInDriverRef,
    _object_id: AudioObjectID,
    _client_process_id: pid_t,
    _address: *const AudioObjectPropertyAddress,
    _out_is_settable: *mut Boolean,
) -> OSStatus {
    let address = *_address;
    let selector = address.mSelector;

    log_msg(&format!("Prism: IsPropertySettable called. Object: {}, Selector: {}", _object_id, selector));

    // Short-circuit: 'rout' is settable everywhere
    if selector == kAudioPrismPropertyRoutingTable {
        *_out_is_settable = 1;
        log_msg("Prism: IsPropertySettable('rout') -> true");
        return 0;
    }

    let res = if selector == kAudioPrismPropertyRoutingTable ||
       selector == kAudioDevicePropertyDeviceName ||
       selector == kAudioObjectPropertyName ||
       selector == kAudioDevicePropertyDataSource || // Add ssrc
       selector == kAudioDevicePropertyNominalSampleRate { // Add nsrt
        *_out_is_settable = 1;
        true
    } else {
        *_out_is_settable = 0;
        false
    };

    log_msg(&format!("Prism: IsPropertySettable called. Object: {}, Selector: {} -> {}", _object_id, selector, res));
    0
}

#[allow(non_upper_case_globals)]
unsafe extern "C" fn get_property_data_size(
    _self: AudioServerPlugInDriverRef,
    object_id: AudioObjectID,
    _client_process_id: pid_t,
    _address: *const AudioObjectPropertyAddress,
    _qualifier_data_size: UInt32,
    _qualifier_data: *const c_void,
    _out_data_size: *mut UInt32,
) -> OSStatus {
    // let driver = _self as *mut PrismDriver; // 今回はconfigアクセス不要ならコメントアウト可
    let address = *_address;
    let selector = address.mSelector;

    // デバッグログ: 必要に応じてコメントアウトしてください
    // log_msg(&format!("Prism: GetPropertyDataSize called. Object: {}, Selector: {}", object_id, selector));

    match object_id {
        // ---------------------------------------------------------------------
        // 1. プラグインオブジェクト
        // ---------------------------------------------------------------------
        id if id == kAudioObjectPlugInObject => {
            if selector == kAudioObjectPropertyCustomPropertyInfoList {
                // プラグイン自体にはカスタムプロパティを持たせない
                *_out_data_size = 0;
            } else if selector == kAudioObjectPropertyBaseClass ||
               selector == kAudioObjectPropertyClass ||
               selector == kAudioObjectPropertyOwner ||
               selector == kAudioPlugInPropertyTranslateUIDToDevice {
                *_out_data_size = std::mem::size_of::<AudioClassID>() as UInt32;
            } else if selector == kAudioObjectPropertyManufacturer ||
                      selector == kAudioPlugInPropertyResourceBundle {
                *_out_data_size = std::mem::size_of::<CFStringRef>() as UInt32;
            } else if selector == kAudioPlugInPropertyDeviceList ||
                      selector == kAudioObjectPropertyOwnedObjects {
                *_out_data_size = std::mem::size_of::<AudioObjectID>() as UInt32;
            } else {
                return kAudioHardwareUnknownPropertyError as OSStatus;
            }
        },

        // ---------------------------------------------------------------------
        // 2. デバイスオブジェクト (ここが本命)
        // ---------------------------------------------------------------------
        DEVICE_ID => {
            // ★ カスタムプロパティ (カタログ)
            if selector == kAudioObjectPropertyCustomPropertyInfoList {
                // Deviceだけが「カスタムプロパティリスト」を持つ
                let size = std::mem::size_of::<AudioServerPlugInCustomPropertyInfo>() as UInt32;
                *_out_data_size = size;
                log_msg(&format!("Prism: Device has 'cust', size={}", size));
                return 0;
            }

            // ★ カスタムプロパティ (実データ: 'rout')
            if selector == kAudioPrismPropertyRoutingTable {
                let size = std::mem::size_of::<PrismRoutingUpdate>() as UInt32;
                *_out_data_size = size;
                log_msg(&format!("Prism: Device has 'rout', size={}", size));
                return 0;
            }

            // --- 標準プロパティ ---
            if selector == kAudioObjectPropertyControlList {
                *_out_data_size = 0;
            } else if selector == kAudioDevicePropertyStreamsIsSettable ||
                      selector == kAudioDevicePropertyClockDomain ||
                      selector == kAudioDevicePropertyClockSource ||
                      selector == kAudioDevicePropertyDataSource ||
                      selector == kAudioObjectPropertyBaseClass ||
                      selector == kAudioObjectPropertyClass ||
                      selector == kAudioObjectPropertyOwner ||
                      selector == kAudioDevicePropertyTransportType ||
                      selector == kAudioDevicePropertyDeviceIsRunning ||
                      selector == kAudioDevicePropertyDeviceCanBeDefaultDevice ||
                      selector == kAudioDevicePropertyDeviceCanBeDefaultSystemDevice ||
                      selector == kAudioDevicePropertySafetyOffset ||
                      selector == kAudioDevicePropertyLatency ||
                      selector == kAudioDevicePropertyDeviceIsAlive ||
                      selector == kAudioDevicePropertyIsHidden ||
                      selector == kAudioDevicePropertyZeroTimeStampPeriod ||
                      selector == kAudioObjectPropertyScope ||
                      selector == kAudioObjectPropertyElement ||
                      selector == kAudioDevicePropertyBufferFrameSize ||
                      selector == kAudioDevicePropertyRingBufferFrameSize {
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioObjectPropertyManufacturer ||
                      selector == kAudioDevicePropertyDeviceUID ||
                      selector == kAudioDevicePropertyModelUID ||
                      selector == kAudioDevicePropertyDeviceName ||
                      selector == kAudioObjectPropertyName {
                *_out_data_size = std::mem::size_of::<CFStringRef>() as UInt32;
            } else if selector == kAudioObjectPropertyOwnedObjects {
                *_out_data_size = (2 * std::mem::size_of::<AudioObjectID>()) as UInt32;
            } else if selector == kAudioDevicePropertyStreams {
                let scope = address.mScope;
                let mut count = 0;
                if scope == kAudioObjectPropertyScopeGlobal || scope == kAudioObjectPropertyScopeInput {
                    count += 1;
                }
                if scope == kAudioObjectPropertyScopeGlobal || scope == kAudioObjectPropertyScopeOutput {
                    count += 1;
                }
                *_out_data_size = (count * std::mem::size_of::<AudioObjectID>()) as UInt32;
            } else if selector == kAudioDevicePropertyNominalSampleRate {
                *_out_data_size = std::mem::size_of::<Float64>() as UInt32;
            } else if selector == kAudioDevicePropertyAvailableNominalSampleRates ||
                      selector == kAudioDevicePropertyBufferFrameSizeRange {
                *_out_data_size = std::mem::size_of::<AudioValueRange>() as UInt32;
            } else {
                // log_msg(&format!("Prism: GetPropertyDataSize Unknown. Object: {}, Selector: {}", object_id, selector));
                return kAudioHardwareUnknownPropertyError as OSStatus;
            }
        },

        // ---------------------------------------------------------------------
        // 3. ストリームオブジェクト
        // ---------------------------------------------------------------------
        INPUT_STREAM_ID | OUTPUT_STREAM_ID => {
            // ★ ストリームはカスタムプロパティを持たない (サイズ 0)
            if selector == kAudioObjectPropertyCustomPropertyInfoList {
                *_out_data_size = 0;
                return 0;
            }

            if selector == kAudioObjectPropertyControlList {
                *_out_data_size = 0;
            } else if selector == kAudioObjectPropertyBaseClass ||
               selector == kAudioObjectPropertyClass ||
               selector == kAudioObjectPropertyOwner ||
               selector == kAudioStreamPropertyDirection ||
               selector == kAudioStreamPropertyTerminalType ||
               selector == kAudioStreamPropertyStartingChannel ||
               selector == kAudioObjectPropertyScope ||
               selector == kAudioObjectPropertyElement {
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioStreamPropertyVirtualFormat ||
                      selector == kAudioStreamPropertyPhysicalFormat {
                *_out_data_size = std::mem::size_of::<AudioStreamBasicDescription>() as UInt32;
            } else if selector == kAudioStreamPropertyPhysicalFormats ||
                      selector == kAudioStreamPropertyAvailableVirtualFormats ||
                      selector == kAudioStreamPropertyAvailablePhysicalFormats {
                *_out_data_size = std::mem::size_of::<AudioStreamRangedDescription>() as UInt32;
            } else {
                return kAudioHardwareUnknownPropertyError as OSStatus;
            }
        },
        _ => return kAudioHardwareBadObjectError as OSStatus,
    }
    0
}

unsafe extern "C" fn get_property_data(
    _self: AudioServerPlugInDriverRef,
    object_id: AudioObjectID,
    _client_process_id: pid_t,
    _address: *const AudioObjectPropertyAddress,
    _qualifier_data_size: UInt32,
    _qualifier_data: *const c_void,
    _in_data_size: UInt32,
    _out_data_size: *mut UInt32,
    _out_data: *mut c_void,
) -> OSStatus {
    let driver = _self as *mut PrismDriver;
    let address = *_address;
    let selector = address.mSelector;

    // ログが多すぎる場合は適宜コメントアウトしてください
    // log_msg(&format!("Prism: GetPropertyData called. Object: {}, Selector: {}", object_id, selector));

    if _out_data.is_null() {
        return kAudioHardwareIllegalOperationError as OSStatus;
    }

    match object_id {
        // ---------------------------------------------------------------------
        // 1. プラグインオブジェクト (Driver PlugIn)
        // ---------------------------------------------------------------------
        id if id == kAudioObjectPlugInObject => {
            if selector == kAudioObjectPropertyCustomPropertyInfoList {
                // プラグイン自体はカスタムプロパティを持たない（デバイスに持たせる）
                *_out_data_size = 0;
                return 0;
            }

            // ... (既存の標準プロパティ処理) ...
            else if selector == kAudioObjectPropertyBaseClass {
                let out = _out_data as *mut AudioClassID;
                *out = kAudioObjectClassID;
                *_out_data_size = std::mem::size_of::<AudioClassID>() as UInt32;
            } else if selector == kAudioObjectPropertyClass {
                let out = _out_data as *mut AudioClassID;
                *out = kAudioPlugInClassID;
                *_out_data_size = std::mem::size_of::<AudioClassID>() as UInt32;
            } else if selector == kAudioObjectPropertyOwner {
                let out = _out_data as *mut AudioObjectID;
                *out = kAudioObjectUnknown;
                *_out_data_size = std::mem::size_of::<AudioObjectID>() as UInt32;
            } else if selector == kAudioObjectPropertyManufacturer {
                let out = _out_data as *mut CFStringRef;
                *out = CFStringCreateWithCString(ptr::null(), "PetitStrawberry\0".as_ptr() as *const i8, kCFStringEncodingUTF8);
                *_out_data_size = std::mem::size_of::<CFStringRef>() as UInt32;
            } else if selector == kAudioPlugInPropertyResourceBundle {
                let out = _out_data as *mut CFStringRef;
                *out = CFStringCreateWithCString(ptr::null(), "com.petitstrawberry.driver.Prism\0".as_ptr() as *const i8, kCFStringEncodingUTF8);
                *_out_data_size = std::mem::size_of::<CFStringRef>() as UInt32;
            } else if selector == kAudioPlugInPropertyDeviceList || selector == kAudioObjectPropertyOwnedObjects {
                let out = _out_data as *mut AudioObjectID;
                *out = DEVICE_ID;
                *_out_data_size = std::mem::size_of::<AudioObjectID>() as UInt32;

                // 遅延通知: プラグインのデバイスリストが取得された後に 'cust' 通知を送る
                if let Some(host) = (*driver).host {
                    if let Some(prop_changed) = (*host).PropertiesChanged {
                        let addr_cust = AudioObjectPropertyAddress {
                            mSelector: kAudioObjectPropertyCustomPropertyInfoList,
                            mScope: kAudioObjectPropertyScopeGlobal,
                            mElement: kAudioObjectPropertyElementMaster,
                        };
                        prop_changed(host, DEVICE_ID, 1, &addr_cust);
                        log_msg("Prism: Late notification sent for Device 'cust' property");
                    }
                }
            } else if selector == kAudioPlugInPropertyTranslateUIDToDevice {
                let mut device_id = kAudioObjectUnknown;
                if _qualifier_data_size == std::mem::size_of::<CFStringRef>() as UInt32 && !_qualifier_data.is_null() {
                     let uid = *(_qualifier_data as *const CFStringRef);
                     let my_uid = CFStringCreateWithCString(ptr::null(), "com.petitstrawberry.driver.Prism.Device\0".as_ptr() as *const i8, kCFStringEncodingUTF8);
                     if CFStringCompare(uid, my_uid, 0) == 0 {
                         device_id = DEVICE_ID;
                     }
                     CFRelease(my_uid as *const c_void);
                }
                let out = _out_data as *mut AudioObjectID;
                *out = device_id;
                *_out_data_size = std::mem::size_of::<AudioObjectID>() as UInt32;
            } else {
                return kAudioHardwareUnknownPropertyError as OSStatus;
            }
        },

        // ---------------------------------------------------------------------
        // 2. デバイスオブジェクト (The Prism Device)
        // ---------------------------------------------------------------------
        DEVICE_ID => {
            // ★ カスタムプロパティの定義 (カタログ)
            if selector == kAudioObjectPropertyCustomPropertyInfoList {
                 log_msg("Prism: GetPropertyData(Device) -> CustomPropertyInfoList");

                 let need = std::mem::size_of::<AudioServerPlugInCustomPropertyInfo>() as UInt32;
                if *_out_data_size < need {
                    return kAudioHardwareBadPropertySizeError as OSStatus;
                }

                 let out = _out_data as *mut AudioServerPlugInCustomPropertyInfo;
                 unsafe {
                     // ここで 'rout' を宣伝する
                     (*out).mSelector = kAudioPrismPropertyRoutingTable;

                     // mPropertyDataType/mQualifierDataType は CoreAudio の定数を使う
                     // BGM と同様に、クライアントがカスタムプロパティを発見しやすいよう
                     // CFPropertyList を使用して宣伝する（多くのクライアントは 'plst' を期待する）
                     (*out).mPropertyDataType = kAudioServerPlugInCustomPropertyDataTypeCFPropertyList;
                     (*out).mQualifierDataType = kAudioServerPlugInCustomPropertyDataTypeNone;
                 }
                 *_out_data_size = need;
                 return 0;

            // ★ カスタムプロパティの実データ ('rout')
            } else if selector == kAudioPrismPropertyRoutingTable {
                log_msg("Prism: GetPropertyData(Device) -> RoutingTable");
                // HALはサイズチェックのために in_data_size=0 で呼ぶことがあるためチェックを緩める
                // しかし書き込みには構造体サイズが必要
                let size = std::mem::size_of::<PrismRoutingUpdate>() as UInt32;

                let out = _out_data as *mut PrismRoutingUpdate;
                unsafe {
                    // 読み込み時はダミーまたは現在の状態を返す
                    *out = PrismRoutingUpdate { pid: 0, channel_offset: 0 };
                }
                *_out_data_size = size;
                return 0;
            }

            // ... (既存の標準プロパティ処理) ...
            else if selector == kAudioObjectPropertyControlList {
                *_out_data_size = 0;
            } else if selector == kAudioObjectPropertyBaseClass {
                let out = _out_data as *mut AudioClassID;
                *out = kAudioObjectClassID;
                *_out_data_size = std::mem::size_of::<AudioClassID>() as UInt32;
            } else if selector == kAudioObjectPropertyClass {
                let out = _out_data as *mut AudioClassID;
                *out = kAudioDeviceClassID;
                *_out_data_size = std::mem::size_of::<AudioClassID>() as UInt32;
            } else if selector == kAudioObjectPropertyOwner {
                let out = _out_data as *mut AudioObjectID;
                *out = kAudioObjectPlugInObject;
                *_out_data_size = std::mem::size_of::<AudioObjectID>() as UInt32;
            } else if selector == kAudioObjectPropertyManufacturer {
                let out = _out_data as *mut CFStringRef;
                *out = CFStringCreateWithCString(ptr::null(), "PetitStrawberry\0".as_ptr() as *const i8, kCFStringEncodingUTF8);
                *_out_data_size = std::mem::size_of::<CFStringRef>() as UInt32;
            } else if selector == kAudioDevicePropertyDeviceUID {
                let out = _out_data as *mut CFStringRef;
                *out = CFStringCreateWithCString(ptr::null(), "com.petitstrawberry.driver.Prism.Device\0".as_ptr() as *const i8, kCFStringEncodingUTF8);
                *_out_data_size = std::mem::size_of::<CFStringRef>() as UInt32;
            } else if selector == kAudioDevicePropertyModelUID {
                let out = _out_data as *mut CFStringRef;
                *out = CFStringCreateWithCString(ptr::null(), "com.petitstrawberry.driver.Prism.Model\0".as_ptr() as *const i8, kCFStringEncodingUTF8);
                *_out_data_size = std::mem::size_of::<CFStringRef>() as UInt32;
            } else if selector == kAudioDevicePropertyDeviceName || selector == kAudioObjectPropertyName {
                let out = _out_data as *mut CFStringRef;
                *out = CFStringCreateWithCString(ptr::null(), "Prism\0".as_ptr() as *const i8, kCFStringEncodingUTF8);
                *_out_data_size = std::mem::size_of::<CFStringRef>() as UInt32;
            } else if selector == kAudioDevicePropertyTransportType {
                let out = _out_data as *mut UInt32;
                *out = kAudioDeviceTransportTypeVirtual;
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioDevicePropertyDeviceIsRunning {
                let out = _out_data as *mut UInt32;
                *out = if (*driver).client_count.load(Ordering::SeqCst) > 0 { 1 } else { 0 };
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioDevicePropertyDeviceIsAlive {
                let out = _out_data as *mut UInt32;
                *out = 1;
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioDevicePropertyIsHidden {
                let out = _out_data as *mut UInt32;
                *out = 0;
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioDevicePropertyStreamsIsSettable {
                let out = _out_data as *mut UInt32;
                *out = 0;
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioDevicePropertyClockDomain {
                let out = _out_data as *mut UInt32;
                *out = 0;
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioDevicePropertyClockSource {
                let out = _out_data as *mut UInt32;
                *out = 0;
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioDevicePropertyDataSource {
                let out = _out_data as *mut UInt32;
                *out = 0;
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioObjectPropertyScope {
                let out = _out_data as *mut UInt32;
                *out = kAudioObjectPropertyScopeGlobal;
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioObjectPropertyElement {
                let out = _out_data as *mut UInt32;
                *out = kAudioObjectPropertyElementMaster;
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioDevicePropertyDeviceCanBeDefaultDevice {
                let out = _out_data as *mut UInt32;
                *out = 1;
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioDevicePropertyDeviceCanBeDefaultSystemDevice {
                let out = _out_data as *mut UInt32;
                *out = 1;
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioDevicePropertySafetyOffset {
                let out = _out_data as *mut UInt32;
                *out = (*driver).config.safety_offset;
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioDevicePropertyLatency {
                let out = _out_data as *mut UInt32;
                *out = 0;
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioDevicePropertyNominalSampleRate {
                let out = _out_data as *mut Float64;
                *out = 48000.0;
                *_out_data_size = std::mem::size_of::<Float64>() as UInt32;
            } else if selector == kAudioDevicePropertyAvailableNominalSampleRates {
                let out = _out_data as *mut AudioValueRange;
                *out = AudioValueRange { mMinimum: 44100.0, mMaximum: 96000.0 };
                *_out_data_size = std::mem::size_of::<AudioValueRange>() as UInt32;
            } else if selector == kAudioDevicePropertyBufferFrameSize {
                let out = _out_data as *mut UInt32;
                *out = (*driver).config.buffer_frame_size;
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioDevicePropertyZeroTimeStampPeriod {
                let out = _out_data as *mut UInt32;
                *out = (*driver).config.zero_timestamp_period;
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioDevicePropertyBufferFrameSizeRange {
                let out = _out_data as *mut AudioValueRange;
                *out = AudioValueRange { mMinimum: 16.0, mMaximum: 4096.0 };
                *_out_data_size = std::mem::size_of::<AudioValueRange>() as UInt32;
            } else if selector == kAudioDevicePropertyRingBufferFrameSize {
                let out = _out_data as *mut UInt32;
                *out = (*driver).config.ring_buffer_frame_size;
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioObjectPropertyOwnedObjects {
                let out = _out_data as *mut AudioObjectID;
                *out.offset(0) = INPUT_STREAM_ID;
                *out.offset(1) = OUTPUT_STREAM_ID;
                *_out_data_size = (2 * std::mem::size_of::<AudioObjectID>()) as UInt32;
            } else if selector == kAudioDevicePropertyStreams {
                let scope = address.mScope;
                let out = _out_data as *mut AudioObjectID;
                let mut count = 0;
                if scope == kAudioObjectPropertyScopeGlobal || scope == kAudioObjectPropertyScopeInput {
                    *out.offset(count) = INPUT_STREAM_ID;
                    count += 1;
                }
                if scope == kAudioObjectPropertyScopeGlobal || scope == kAudioObjectPropertyScopeOutput {
                    *out.offset(count) = OUTPUT_STREAM_ID;
                    count += 1;
                }
                *_out_data_size = (count as usize * std::mem::size_of::<AudioObjectID>()) as UInt32;
            } else {
                return kAudioHardwareUnknownPropertyError as OSStatus;
            }
        },

        // ---------------------------------------------------------------------
        // 3. ストリームオブジェクト
        // ---------------------------------------------------------------------
        INPUT_STREAM_ID | OUTPUT_STREAM_ID => {
             // ストリームはカスタムプロパティを持たない
            if selector == kAudioObjectPropertyCustomPropertyInfoList {
                *_out_data_size = 0;
                return 0;
            }
            // ... (既存の処理) ...
            else if selector == kAudioObjectPropertyControlList {
                *_out_data_size = 0;
            } else if selector == kAudioObjectPropertyBaseClass {
                let out = _out_data as *mut AudioClassID;
                *out = kAudioObjectClassID;
                *_out_data_size = std::mem::size_of::<AudioClassID>() as UInt32;
            } else if selector == kAudioObjectPropertyClass {
                let out = _out_data as *mut AudioClassID;
                *out = kAudioStreamClassID;
                *_out_data_size = std::mem::size_of::<AudioClassID>() as UInt32;
            } else if selector == kAudioObjectPropertyOwner {
                let out = _out_data as *mut AudioObjectID;
                *out = DEVICE_ID;
                *_out_data_size = std::mem::size_of::<AudioObjectID>() as UInt32;
            } else if selector == kAudioObjectPropertyScope {
                let out = _out_data as *mut UInt32;
                *out = if object_id == INPUT_STREAM_ID { kAudioObjectPropertyScopeInput } else { kAudioObjectPropertyScopeOutput };
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioObjectPropertyElement {
                let out = _out_data as *mut UInt32;
                *out = kAudioObjectPropertyElementMaster;
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioStreamPropertyDirection {
                let out = _out_data as *mut UInt32;
                *out = if object_id == INPUT_STREAM_ID { 1 } else { 0 };
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioStreamPropertyTerminalType {
                let out = _out_data as *mut UInt32;
                *out = if object_id == INPUT_STREAM_ID { 0x6D696320 } else { 0x73706B72 };
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioStreamPropertyStartingChannel {
                let out = _out_data as *mut UInt32;
                *out = 1;
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioStreamPropertyVirtualFormat || selector == kAudioStreamPropertyPhysicalFormat {
                let out = _out_data as *mut AudioStreamBasicDescription;
                *out = AudioStreamBasicDescription {
                    mSampleRate: 48000.0,
                    mFormatID: kAudioFormatLinearPCM,
                    mFormatFlags: kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked,
                    mBytesPerPacket: 4 * (*driver).config.num_channels,
                    mFramesPerPacket: 1,
                    mBytesPerFrame: 4 * (*driver).config.num_channels,
                    mChannelsPerFrame: (*driver).config.num_channels,
                    mBitsPerChannel: 32,
                    mReserved: 0,
                };
                *_out_data_size = std::mem::size_of::<AudioStreamBasicDescription>() as UInt32;
            } else if selector == kAudioStreamPropertyPhysicalFormats ||
                      selector == kAudioStreamPropertyAvailableVirtualFormats ||
                      selector == kAudioStreamPropertyAvailablePhysicalFormats {
                let out = _out_data as *mut AudioStreamRangedDescription;
                *out = AudioStreamRangedDescription {
                    mFormat: AudioStreamBasicDescription {
                        mSampleRate: 48000.0,
                        mFormatID: kAudioFormatLinearPCM,
                        mFormatFlags: kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked,
                        mBytesPerPacket: 4 * (*driver).config.num_channels,
                        mFramesPerPacket: 1,
                        mBytesPerFrame: 4 * (*driver).config.num_channels,
                        mChannelsPerFrame: (*driver).config.num_channels,
                        mBitsPerChannel: 32,
                        mReserved: 0,
                    },
                    mSampleRateRange: AudioValueRange { mMinimum: 48000.0, mMaximum: 48000.0 },
                };
                *_out_data_size = std::mem::size_of::<AudioStreamRangedDescription>() as UInt32;
            } else {
                return kAudioHardwareUnknownPropertyError as OSStatus;
            }
        },
        _ => return kAudioHardwareBadObjectError as OSStatus,
    }
    0
}

unsafe extern "C" fn set_property_data(
    _self: AudioServerPlugInDriverRef,
    _object_id: AudioObjectID,
    _client_process_id: pid_t,
    _address: *const AudioObjectPropertyAddress,
    _qualifier_data_size: UInt32,
    _qualifier_data: *const c_void,
    _in_data_size: UInt32,
    _in_data: *const c_void,
) -> OSStatus {
    let driver = _self as *mut PrismDriver;
    let address = *_address;
    let selector = address.mSelector;

    log_msg(&format!("Prism: SetPropertyData called. Object: {}, Selector: {}", _object_id, selector));

    if selector == kAudioPrismPropertyRoutingTable {
        if _in_data_size != std::mem::size_of::<PrismRoutingUpdate>() as UInt32 {
            return kAudioHardwareBadPropertySizeError as OSStatus;
        }
        let update = *(_in_data as *const PrismRoutingUpdate);
        let pid = update.pid;
        let offset = update.channel_offset;

        log_msg(&format!("Prism: SetPropertyData ROUT: PID={}, Offset={}", pid, offset));

        if pid != 0 {
            // Apply update
            let mut found = false;
            let driver_ref = &*driver;
            let slots = &driver_ref.client_slots;
            for j in 0..MAX_CLIENTS {
                    let slot = &slots[j];
                    if slot.pid.load(Ordering::Acquire) == pid {
                        slot.channel_offset.store(offset as usize, Ordering::Release);
                        log_msg(&format!("Prism: Routing Update via ROUT. PID={}, Offset={}", pid, offset));
                        found = true;
                    }
            }
            if !found {
                log_msg(&format!("Prism: Routing Update via ROUT Failed. PID={} not found", pid));
            }
        }
        return 0;
    } else if selector == kAudioDevicePropertyDeviceName || selector == kAudioObjectPropertyName {
        if _in_data_size != std::mem::size_of::<CFStringRef>() as UInt32 {
            return kAudioHardwareBadPropertySizeError as OSStatus;
        }
        let name_ref = *(_in_data as *const CFStringRef);

        // Convert CFString to Rust String
        let mut buf = [0u8; 256];
        let success = CFStringGetCString(name_ref, buf.as_mut_ptr() as *mut i8, 256, kCFStringEncodingUTF8);

        if success != 0 {
            let c_str = std::ffi::CStr::from_ptr(buf.as_ptr() as *const i8);
            if let Ok(s) = c_str.to_str() {
                log_msg(&format!("Prism: SetPropertyData DeviceName: {}", s));

                // Parse "PID:1234,Offset:2"
                // Simple parsing
                if s.starts_with("PID:") {
                    let parts: Vec<&str> = s.split(',').collect();
                    let mut pid = 0;
                    let mut offset = 0;

                    for part in parts {
                        if part.starts_with("PID:") {
                            if let Ok(p) = part[4..].parse::<i32>() {
                                pid = p;
                            }
                        } else if part.starts_with("Offset:") {
                            if let Ok(o) = part[7..].parse::<u32>() {
                                offset = o;
                            }
                        }
                    }

                    if pid != 0 {
                        // Apply update
                        let mut found = false;
                        let driver_ref = &*driver;
                        let slots = &driver_ref.client_slots;
                        for j in 0..MAX_CLIENTS {
                             let slot = &slots[j];
                             if slot.pid.load(Ordering::Acquire) == pid {
                                 slot.channel_offset.store(offset as usize, Ordering::Release);
                                 log_msg(&format!("Prism: Routing Update via Name. PID={}, Offset={}", pid, offset));
                                 found = true;
                             }
                        }
                        if !found {
                            log_msg(&format!("Prism: Routing Update via Name Failed. PID={} not found", pid));
                        }
                    }
                }
            }
        }
        return 0;
    } else if selector == kAudioDevicePropertyDataSource || selector == kAudioDevicePropertyNominalSampleRate {
        // Trigger Command Read
        log_msg("Prism: DataSource/SampleRate set. Reading command from /tmp/prism_command.txt");
        if let Ok(content) = std::fs::read_to_string("/tmp/prism_command.txt") {
             log_msg(&format!("Prism: Command content: {}", content));
             // Parse "PID:1234,Offset:2"
             if content.starts_with("PID:") {
                    let parts: Vec<&str> = content.split(',').collect();
                    let mut pid = 0;
                    let mut offset = 0;

                    for part in parts {
                        if part.starts_with("PID:") {
                            if let Ok(p) = part[4..].trim().parse::<i32>() {
                                pid = p;
                            }
                        } else if part.starts_with("Offset:") {
                            if let Ok(o) = part[7..].trim().parse::<u32>() {
                                offset = o;
                            }
                        }
                    }

                    if pid != 0 {
                        // Apply update
                        let mut found = false;
                        let driver_ref = &*driver;
                        let slots = &driver_ref.client_slots;
                        for j in 0..MAX_CLIENTS {
                             let slot = &slots[j];
                             if slot.pid.load(Ordering::Acquire) == pid {
                                 slot.channel_offset.store(offset as usize, Ordering::Release);
                                 log_msg(&format!("Prism: Routing Update via File. PID={}, Offset={}", pid, offset));
                                 found = true;
                             }
                        }
                        if !found {
                            log_msg(&format!("Prism: Routing Update via File Failed. PID={} not found", pid));
                        }
                    }
             }
        }
        return 0;
    }

    kAudioHardwareUnknownPropertyError as OSStatus
}

// --- Driver Callbacks ---

#[allow(deprecated)]
unsafe extern "C" fn start_io(
    _self: AudioServerPlugInDriverRef,
    _device_id: AudioObjectID,
    _client_id: UInt32,
) -> OSStatus {
    log_msg("Prism: StartIO called");
    let driver = _self as *mut PrismDriver;

    let prev_count = (*driver).client_count.fetch_add(1, Ordering::SeqCst);
    if prev_count == 0 {
        let now = libc::mach_absolute_time();
        (*driver).anchor_host_time.store(now, Ordering::SeqCst);
        (*driver).num_time_stamps.store(0, Ordering::SeqCst);
        (*driver).write_pos.store(0, Ordering::SeqCst);
        (*driver).read_pos.store(0, Ordering::SeqCst);

        if let Some(host) = (*driver).host {
             let address = AudioObjectPropertyAddress {
                mSelector: kAudioDevicePropertyDeviceIsRunning,
                mScope: kAudioObjectPropertyScopeGlobal,
                mElement: kAudioObjectPropertyElementMaster,
            };
            if let Some(prop_changed) = (*host).PropertiesChanged {
                prop_changed(host, _device_id, 1, &address);
            }

            // Also notify about CustomPropertyInfoList to force refresh
            let cust_address = AudioObjectPropertyAddress {
                mSelector: kAudioObjectPropertyCustomPropertyInfoList,
                mScope: kAudioObjectPropertyScopeGlobal,
                mElement: kAudioObjectPropertyElementMaster,
            };
            if let Some(prop_changed) = (*host).PropertiesChanged {
                prop_changed(host, _device_id, 1, &cust_address);
                log_msg("Prism: Notified PropertiesChanged for CustomPropertyInfoList");
            }
        }
    }
    0
}

#[allow(deprecated)]
unsafe extern "C" fn stop_io(
    _self: AudioServerPlugInDriverRef,
    _device_id: AudioObjectID,
    _client_id: UInt32,
) -> OSStatus {
    // log_msg("Prism: StopIO called");
    let driver = _self as *mut PrismDriver;

    let prev_count = (*driver).client_count.fetch_sub(1, Ordering::SeqCst);
    if prev_count == 1 {
        (*driver).anchor_host_time.store(0, Ordering::SeqCst);

        if let Some(host) = (*driver).host {
             let address = AudioObjectPropertyAddress {
                mSelector: kAudioDevicePropertyDeviceIsRunning,
                mScope: kAudioObjectPropertyScopeGlobal,
                mElement: kAudioObjectPropertyElementMaster,
            };
            if let Some(prop_changed) = (*host).PropertiesChanged {
                prop_changed(host, _device_id, 1, &address);
            }
        }
    }
    0
}

#[allow(deprecated)]
unsafe extern "C" fn get_zero_timestamp(
    _self: AudioServerPlugInDriverRef,
    _device_id: AudioObjectID,
    _client_id: UInt32,
    out_sample_time: *mut Float64,
    out_host_time: *mut UInt64,
    out_seed: *mut UInt64,
) -> OSStatus {
    let driver = _self as *mut PrismDriver;
    let anchor = (*driver).anchor_host_time.load(Ordering::SeqCst);

    if anchor == 0 {
        *out_sample_time = 0.0;
        *out_host_time = 0;
        *out_seed = 0;
        return 0;
    }

    let current_host_time = libc::mach_absolute_time();
    let period_frames = (*driver).config.zero_timestamp_period as f64; // kZeroTimeStampPeriod
    let host_ticks_per_period = (*driver).host_ticks_per_frame * period_frames;

    // Calculate the next zero crossing based on anchor time
    // We want the smallest N such that anchor + N * period > current_time
    let elapsed_ticks = if current_host_time > anchor {
        current_host_time - anchor
    } else {
        0
    };

    let num_periods = (elapsed_ticks as f64 / host_ticks_per_period).floor() as u64;
    let next_period = num_periods + 1;

    *out_sample_time = next_period as f64 * period_frames;
    *out_host_time = anchor + (next_period as f64 * host_ticks_per_period) as u64;
    *out_seed = 1;

    0
}
unsafe extern "C" fn will_do_io_operation(
    _self: AudioServerPlugInDriverRef,
    _device_id: AudioObjectID,
    _client_id: UInt32,
    _operation_id: UInt32,
    _out_will_do: *mut Boolean,
    _out_will_do_in_place: *mut Boolean,
) -> OSStatus {
    *_out_will_do = 1;
    *_out_will_do_in_place = 1;
    0
}

unsafe extern "C" fn begin_io_operation(
    _self: AudioServerPlugInDriverRef,
    _device_id: AudioObjectID,
    _client_id: UInt32,
    _operation_id: UInt32,
    _io_buffer_frame_size: UInt32,
    _io_cycle_info: *const AudioServerPlugInIOCycleInfo,
) -> OSStatus {
    0
}

unsafe extern "C" fn do_io_operation(
    _self: AudioServerPlugInDriverRef,
    _device_id: AudioObjectID,
    _stream_id: AudioObjectID,
    _client_id: UInt32,
    _operation_id: UInt32,
    _io_buffer_frame_size: UInt32,
    _io_cycle_info: *const AudioServerPlugInIOCycleInfo,
    _io_main_buffer: *mut c_void,
    _io_secondary_buffer: *mut c_void,
) -> OSStatus {
    let driver = _self as *mut PrismDriver;
    let loopback_buffer = &mut (*driver).loopback_buffer;
    let frames = _io_buffer_frame_size as usize;
    let channels = (*driver).config.num_channels as usize;
    let buffer_len = loopback_buffer.len(); // Total samples in buffer
    let buffer_frames = buffer_len / channels; // Total frames in buffer

    if _io_cycle_info.is_null() {
        return kAudioHardwareIllegalOperationError as OSStatus;
    }
    #[allow(unused_variables)]
    let cycle_info = &*_io_cycle_info;

    if _operation_id == kAudioServerPlugInIOOperationWriteMix {
        if !_io_main_buffer.is_null() {
            // Direct Indexing
            let idx = (_client_id as usize) & (MAX_CLIENTS - 1);
            let slots = &(*driver).client_slots;
            let slot = &slots[idx];

            // Verify Client ID
            if slot.client_id.load(Ordering::Acquire) != _client_id {
                return 0;
            }
            let channel_offset = slot.channel_offset.load(Ordering::Relaxed);

            let input = _io_main_buffer as *const f32;
            // Use mOutputTime to determine write position in ring buffer
            // This ensures all clients writing to the same time slot write to the same buffer index
            let sample_time = cycle_info.mOutputTime.mSampleTime as usize;
            let w_pos = sample_time % buffer_frames;

            // Calculate how many frames we can write before wrapping
            let frames_until_wrap = buffer_frames - w_pos;
            let input_channels = channels; // The buffer is 16ch interleaved

            if frames <= frames_until_wrap {
                // No wrapping needed
                for i in 0..frames {
                    let in_l = *input.add(i * input_channels + 0);
                    let in_r = *input.add(i * input_channels + 1);

                    let dst_idx = (w_pos + i) * channels + channel_offset;
                    if dst_idx + 1 < buffer_len {
                        loopback_buffer[dst_idx] += in_l;
                        loopback_buffer[dst_idx + 1] += in_r;
                    }
                }
            } else {
                // Wrapping needed
                for i in 0..frames_until_wrap {
                    let in_l = *input.add(i * input_channels + 0);
                    let in_r = *input.add(i * input_channels + 1);
                    let dst_idx = (w_pos + i) * channels + channel_offset;
                    if dst_idx + 1 < buffer_len {
                        loopback_buffer[dst_idx] += in_l;
                        loopback_buffer[dst_idx + 1] += in_r;
                    }
                }

                let remainder = frames - frames_until_wrap;
                for i in 0..remainder {
                    let src_idx = frames_until_wrap + i;
                    let in_l = *input.add(src_idx * input_channels + 0);
                    let in_r = *input.add(src_idx * input_channels + 1);
                    let dst_idx = i * channels + channel_offset;
                     if dst_idx + 1 < buffer_len {
                        loopback_buffer[dst_idx] += in_l;
                        loopback_buffer[dst_idx + 1] += in_r;
                    }
                }
            }
        }
    } else if _operation_id == kAudioServerPlugInIOOperationReadInput {
        if !_io_main_buffer.is_null() {
            let output = _io_main_buffer as *mut f32;
            // Use mInputTime to determine read position
            let sample_time = cycle_info.mInputTime.mSampleTime as usize;
            let r_pos = sample_time % buffer_frames;

            // Calculate how many frames we can read before wrapping
            let frames_until_wrap = buffer_frames - r_pos;

            if frames <= frames_until_wrap {
                // No wrapping needed
                let src_ptr = loopback_buffer.as_ptr().add(r_pos * channels);
                let dst_ptr = output;
                ptr::copy_nonoverlapping(src_ptr, dst_ptr, frames * channels);

                // Destructive Read: Clear the buffer after reading to prevent old data from looping
                // This is essential for OMNIBUS-style routing where channels might not be overwritten every cycle.
                ptr::write_bytes(loopback_buffer.as_mut_ptr().add(r_pos * channels), 0, frames * channels);
            } else {
                // Wrapping needed
                // 1. Read until end
                let src_ptr1 = loopback_buffer.as_ptr().add(r_pos * channels);
                let dst_ptr1 = output;
                ptr::copy_nonoverlapping(src_ptr1, dst_ptr1, frames_until_wrap * channels);

                // Clear part 1
                ptr::write_bytes(loopback_buffer.as_mut_ptr().add(r_pos * channels), 0, frames_until_wrap * channels);

                // 2. Read remainder from start
                let remainder = frames - frames_until_wrap;
                let src_ptr2 = loopback_buffer.as_ptr(); // Start of buffer
                let dst_ptr2 = output.add(frames_until_wrap * channels);
                ptr::copy_nonoverlapping(src_ptr2, dst_ptr2, remainder * channels);

                // Clear part 2
                ptr::write_bytes(loopback_buffer.as_mut_ptr(), 0, remainder * channels);
            }
        }
    }
    0
}unsafe extern "C" fn end_io_operation(
    _self: AudioServerPlugInDriverRef,
    _device_id: AudioObjectID,
    _client_id: UInt32,
    _operation_id: UInt32,
    _io_buffer_frame_size: UInt32,
    _io_cycle_info: *const AudioServerPlugInIOCycleInfo,
) -> OSStatus {
    0
}

// Helper for logging
fn log_msg(msg: &str) {
    use std::ffi::CString;
    unsafe {
        // syslog(LOG_USER, ...)
        let c_msg = CString::new(msg).unwrap_or_else(|_| CString::new("prism: log error").unwrap());
        libc::syslog(libc::LOG_USER | libc::LOG_INFO, c_msg.as_ptr());
    }
}

// V-Table storage
static mut DRIVER_VTABLE: AudioServerPlugInDriverInterface = AudioServerPlugInDriverInterface {
    _reserved: ptr::null_mut(),
    QueryInterface: Some(query_interface),
    AddRef: Some(add_ref),
    Release: Some(release),
    Initialize: Some(initialize),
    CreateDevice: Some(create_device),
    DestroyDevice: Some(destroy_device),
    AddDeviceClient: Some(add_device_client),
    RemoveDeviceClient: Some(remove_device_client),
    PerformDeviceConfigurationChange: Some(perform_device_configuration_change),
    AbortDeviceConfigurationChange: Some(abort_device_configuration_change),
    HasProperty: Some(has_property),
    IsPropertySettable: Some(is_property_settable),
    GetPropertyDataSize: Some(get_property_data_size),
    GetPropertyData: Some(get_property_data),
    SetPropertyData: Some(set_property_data),
    StartIO: Some(start_io),
    StopIO: Some(stop_io),
    GetZeroTimeStamp: Some(get_zero_timestamp),
    WillDoIOOperation: Some(will_do_io_operation),
    BeginIOOperation: Some(begin_io_operation),
    DoIOOperation: Some(do_io_operation),
    EndIOOperation: Some(end_io_operation),
};

pub fn create_driver() -> *mut PrismDriver {
    unsafe {
        if DRIVER_INSTANCE.is_null() {
            let host_ticks_per_second = get_host_ticks_per_second();
            let sample_rate = 48000.0; // Must match what we report in GetPropertyData
            let host_ticks_per_frame = host_ticks_per_second / sample_rate;

            let config = PrismConfig::load();
            let buffer_size = 65536 * config.num_channels as usize; // 65536 frames * channels

            let mut client_slots = Vec::with_capacity(MAX_CLIENTS);
            for _ in 0..MAX_CLIENTS {
                client_slots.push(ClientSlot {
                    client_id: AtomicU32::new(0),
                    channel_offset: AtomicUsize::new(0),
                    pid: AtomicI32::new(0),
                });
            }

            let driver = Box::new(PrismDriver {
                _vtable: &raw const DRIVER_VTABLE,
                ref_count: AtomicU32::new(1),
                host: None,
                anchor_host_time: AtomicU64::new(0),
                num_time_stamps: AtomicU64::new(0),
                host_ticks_per_frame,
                client_count: AtomicU32::new(0),
                phase: 0.0,
                loopback_buffer: vec![0.0; buffer_size],
                config,
                _pad1: [0; 64],
                write_pos: AtomicUsize::new(0),
                _pad2: [0; 64],
                read_pos: AtomicUsize::new(0),
                client_slots,
            });
            DRIVER_INSTANCE = Box::into_raw(driver);
        } else {
            // Increment ref count if we were doing real ref counting,
            // but for a singleton driver, we usually just return the instance.
            (*DRIVER_INSTANCE).ref_count.fetch_add(1, Ordering::Relaxed);
        }
        DRIVER_INSTANCE
    }
}

#[repr(C)]
#[allow(non_snake_case)]
struct PrismClientInfo {
    mClientID: UInt32,
    mProcessID: pid_t,
    mIsNativeEndian: Boolean,
    mBundleID: CFStringRef,
}
