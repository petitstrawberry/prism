use coreaudio_sys::*;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

// Define the Host Interface struct locally since coreaudio-sys seems to treat it as opaque or we are having trouble dereferencing it.
// This layout must match the C definition of AudioServerPlugInHostInterface.
// Removed PrismHostInterface as it is not used yet.

// UUID for the driver interface (kAudioServerPlugInDriverInterfaceUUID)
// This should match what is expected by Core Audio.
// In coreaudio-sys, this might be available as a constant, but often we need to construct it.
// For now, we'll use the standard UUID for the driver interface.

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
    pub write_pos: AtomicUsize,
    pub read_pos: AtomicUsize,
}

// The singleton instance of our driver
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
    // log_msg("Prism: Initialize called");
    let driver = _self as *mut PrismDriver;
    (*driver).host = Some(host);
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
    0
}

unsafe extern "C" fn remove_device_client(
    _self: AudioServerPlugInDriverRef,
    _device_id: AudioObjectID,
    _client_id: *const AudioServerPlugInClientInfo,
) -> OSStatus {
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
unsafe extern "C" fn has_property(
    _self: AudioServerPlugInDriverRef,
    object_id: AudioObjectID,
    _client_process_id: pid_t,
    _address: *const AudioObjectPropertyAddress,
) -> Boolean {
    let address = *_address;
    let selector = address.mSelector;

    let res = match object_id {
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
                true
            } else {
                false
            }
        },
        DEVICE_ID => {
            if selector == kAudioObjectPropertyBaseClass ||
               selector == kAudioObjectPropertyClass ||
               selector == kAudioObjectPropertyOwner ||
               selector == kAudioObjectPropertyManufacturer ||
               selector == kAudioObjectPropertyOwnedObjects ||
               selector == kAudioObjectPropertyControlList ||
               selector == kAudioObjectPropertyCustomPropertyInfoList ||
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
               selector == kAudioObjectPropertyScope ||
               selector == kAudioObjectPropertyElement ||
               selector == kAudioDevicePropertyBufferFrameSize {
                true
            } else {
                false
            }
        },
        INPUT_STREAM_ID | OUTPUT_STREAM_ID => {
            if selector == kAudioObjectPropertyBaseClass ||
               selector == kAudioObjectPropertyClass ||
               selector == kAudioObjectPropertyOwner ||
               selector == kAudioObjectPropertyControlList ||
               selector == kAudioObjectPropertyCustomPropertyInfoList ||
               selector == kAudioStreamPropertyDirection ||
               selector == kAudioStreamPropertyTerminalType ||
               selector == kAudioStreamPropertyStartingChannel ||
               selector == kAudioObjectPropertyScope ||
               selector == kAudioObjectPropertyElement {
                true
            } else {
                false
            }
        },
        _ => {
            log_msg(&format!("Prism: HasProperty called. Object: {}, Selector: {} -> false", object_id, selector));
            false
        }
    };

    // log_msg(&format!("Prism: HasProperty called. Object: {}, Selector: {} -> {}", object_id, selector, res));
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
    *_out_is_settable = 0;
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
    let address = *_address;
    let selector = address.mSelector;
    // log_msg(&format!("Prism: GetPropertyDataSize called. Object: {}, Selector: {}", object_id, selector));

    match object_id {
        id if id == kAudioObjectPlugInObject => {
            if selector == kAudioObjectPropertyBaseClass ||
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
            } else if selector == kAudioObjectPropertyCustomPropertyInfoList {
                *_out_data_size = 0;
            } else {
                log_msg(&format!("Prism: GetPropertyDataSize called. Object: {}, Selector: {}", object_id, selector));
                return kAudioHardwareUnknownPropertyError as OSStatus;
            }
        },
        DEVICE_ID => {
            if selector == kAudioObjectPropertyControlList || selector == kAudioObjectPropertyCustomPropertyInfoList {
                *_out_data_size = 0;
            } else if selector == kAudioDevicePropertyStreamsIsSettable || selector == kAudioDevicePropertyClockDomain || selector == kAudioDevicePropertyClockSource {
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioObjectPropertyBaseClass ||
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
               selector == kAudioObjectPropertyElement {
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
            } else if selector == kAudioDevicePropertyRingBufferFrameSize {
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else {
                log_msg(&format!("Prism: GetPropertyDataSize called. Object: {}, Selector: {}", object_id, selector));
                return kAudioHardwareUnknownPropertyError as OSStatus;
            }
        },
        INPUT_STREAM_ID | OUTPUT_STREAM_ID => {
            if selector == kAudioObjectPropertyControlList || selector == kAudioObjectPropertyCustomPropertyInfoList {
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
                log_msg(&format!("Prism: GetPropertyDataSize called. Object: {}, Selector: {}", object_id, selector));
                return kAudioHardwareUnknownPropertyError as OSStatus;
            }
        },
        _ => return kAudioHardwareBadObjectError as OSStatus,
    }
    0
}

#[allow(non_upper_case_globals)]
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
    // log_msg(&format!("Prism: GetPropertyData called. Object: {}, Selector: {}", object_id, selector));

    if _out_data.is_null() {
        return kAudioHardwareIllegalOperationError as OSStatus;
    }

    match object_id {
        id if id == kAudioObjectPlugInObject => {
            if selector == kAudioObjectPropertyCustomPropertyInfoList {
                *_out_data_size = 0;
            } else if selector == kAudioObjectPropertyBaseClass {
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
            } else if selector == kAudioPlugInPropertyTranslateUIDToDevice {
                // Check if the UID matches our device UID
                // For simplicity, we assume it matches if we are asked, or we should check the qualifier.
                // But usually the HAL asks this with the UID in the qualifier?
                // Wait, the spec says the UID is passed as the qualifier?
                // "The qualifier is a CFStringRef that contains the UID."

                // Let's check if we have qualifier data
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
                log_msg(&format!("Prism: GetPropertyData called. Object: {}, Selector: {}", object_id, selector));
                return kAudioHardwareUnknownPropertyError as OSStatus;
            }
        },
        DEVICE_ID => {
            if selector == kAudioObjectPropertyControlList {
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
                *out = 0; // Internal clock
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
                *out = 256;
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
                *out = AudioValueRange { mMinimum: 48000.0, mMaximum: 48000.0 };
                *_out_data_size = std::mem::size_of::<AudioValueRange>() as UInt32;
            } else if selector == kAudioDevicePropertyBufferFrameSize {
                let out = _out_data as *mut UInt32;
                *out = 1024;
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioDevicePropertyZeroTimeStampPeriod {
                let out = _out_data as *mut UInt32;
                *out = 1024;
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioDevicePropertyBufferFrameSizeRange {
                let out = _out_data as *mut AudioValueRange;
                *out = AudioValueRange { mMinimum: 16.0, mMaximum: 4096.0 };
                *_out_data_size = std::mem::size_of::<AudioValueRange>() as UInt32;
            } else if selector == kAudioDevicePropertyRingBufferFrameSize {
                let out = _out_data as *mut UInt32;
                *out = 1024;
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
                log_msg(&format!("Prism: GetPropertyData called. Object: {}, Selector: {}", object_id, selector));
                return kAudioHardwareUnknownPropertyError as OSStatus;
            }
        },
        INPUT_STREAM_ID | OUTPUT_STREAM_ID => {
            if selector == kAudioObjectPropertyControlList || selector == kAudioObjectPropertyCustomPropertyInfoList {
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
                // 'mic ' = 0x6D696320, 'spkr' = 0x73706B72
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
                    mBytesPerPacket: 8,
                    mFramesPerPacket: 1,
                    mBytesPerFrame: 8,
                    mChannelsPerFrame: 2,
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
                        mBytesPerPacket: 8,
                        mFramesPerPacket: 1,
                        mBytesPerFrame: 8,
                        mChannelsPerFrame: 2,
                        mBitsPerChannel: 32,
                        mReserved: 0,
                    },
                    mSampleRateRange: AudioValueRange { mMinimum: 48000.0, mMaximum: 48000.0 },
                };
                *_out_data_size = std::mem::size_of::<AudioStreamRangedDescription>() as UInt32;
            } else {
                log_msg(&format!("Prism: GetPropertyData called. Object: {}, Selector: {}", object_id, selector));
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
    0
}

#[allow(deprecated)]
unsafe extern "C" fn start_io(
    _self: AudioServerPlugInDriverRef,
    _device_id: AudioObjectID,
    _client_id: UInt32,
) -> OSStatus {
    // log_msg("Prism: StartIO called");
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
    let period_frames = 1024.0; // kZeroTimeStampPeriod
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
    let channels = 2;
    let buffer_len = loopback_buffer.len();
    let buffer_frames = buffer_len / channels;

    if _operation_id == kAudioServerPlugInIOOperationWriteMix {
        if !_io_main_buffer.is_null() {
            let input = _io_main_buffer as *const f32;
            let mut w_pos = (*driver).write_pos.load(Ordering::Acquire);
            
            for i in 0..frames {
                for c in 0..channels {
                    let sample = *input.add(i * channels + c);
                    loopback_buffer[w_pos * channels + c] = sample;
                }
                w_pos = (w_pos + 1) % buffer_frames;
            }
            (*driver).write_pos.store(w_pos, Ordering::Release);
        }
    } else if _operation_id == kAudioServerPlugInIOOperationReadInput {
        if !_io_main_buffer.is_null() {
            let output = _io_main_buffer as *mut f32;
            let mut r_pos = (*driver).read_pos.load(Ordering::Acquire);
            
            for i in 0..frames {
                for c in 0..channels {
                    let sample = loopback_buffer[r_pos * channels + c];
                    *output.add(i * channels + c) = sample;
                }
                r_pos = (r_pos + 1) % buffer_frames;
            }
            (*driver).read_pos.store(r_pos, Ordering::Release);
        }
    }
    0
}

unsafe extern "C" fn end_io_operation(
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
fn log_msg(_msg: &str) {
    // use std::io::Write;
    // // Use a fixed path in /tmp to ensure we can write to it and find it.
    // // Ignoring errors as we can't do much if logging fails.
    // if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open("/tmp/prism.log") {
    //     let _ = writeln!(file, "{}", msg);
    // }
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

            let buffer_size = 65536 * 2; // 65536 frames, 2 channels
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
                write_pos: AtomicUsize::new(0),
                read_pos: AtomicUsize::new(0),
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
