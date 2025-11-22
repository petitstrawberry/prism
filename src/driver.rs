use coreaudio_sys::*;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};

// Define the Host Interface struct locally since coreaudio-sys seems to treat it as opaque or we are having trouble dereferencing it.
// This layout must match the C definition of AudioServerPlugInHostInterface.
#[repr(C)]
#[allow(non_snake_case)]
pub struct PrismHostInterface {
    pub _reserved: *mut c_void,
    pub QueryInterface: Option<unsafe extern "C" fn(inDriver: *mut c_void, inUUID: *const c_void, outInterface: *mut *mut c_void) -> HRESULT>,
    pub AddRef: Option<unsafe extern "C" fn(inDriver: *mut c_void) -> ULONG>,
    pub Release: Option<unsafe extern "C" fn(inDriver: *mut c_void) -> ULONG>,
    pub CreateDevice: Option<unsafe extern "C" fn(inHost: AudioServerPlugInHostRef, inDescription: CFDictionaryRef, outDeviceID: *mut AudioObjectID) -> OSStatus>,
    pub DestroyDevice: Option<unsafe extern "C" fn(inHost: AudioServerPlugInHostRef, inDeviceID: AudioObjectID) -> OSStatus>,
    pub AddDeviceClient: Option<unsafe extern "C" fn(inHost: AudioServerPlugInHostRef, inDeviceID: AudioObjectID, inClientInfo: *const AudioServerPlugInClientInfo) -> OSStatus>,
    pub RemoveDeviceClient: Option<unsafe extern "C" fn(inHost: AudioServerPlugInHostRef, inDeviceID: AudioObjectID, inClientInfo: *const AudioServerPlugInClientInfo) -> OSStatus>,
    pub PerformDeviceConfigurationChange: Option<unsafe extern "C" fn(inHost: AudioServerPlugInHostRef, inDeviceID: AudioObjectID, inChangeAction: u64, inChangeInfo: *mut c_void) -> OSStatus>,
    pub RequestDeviceConfigurationChange: Option<unsafe extern "C" fn(inHost: AudioServerPlugInHostRef, inDeviceID: AudioObjectID, inChangeAction: u64, inChangeInfo: *mut c_void) -> OSStatus>,
}

// UUID for the driver interface (kAudioServerPlugInDriverInterfaceUUID)
// This should match what is expected by Core Audio.
// In coreaudio-sys, this might be available as a constant, but often we need to construct it.
// For now, we'll use the standard UUID for the driver interface.

#[repr(C)]
pub struct PrismDriver {
    pub _vtable: *const AudioServerPlugInDriverInterface,
    pub ref_count: AtomicU32,
    pub host: Option<AudioServerPlugInHostRef>,
}

// The singleton instance of our driver
static mut DRIVER_INSTANCE: *mut PrismDriver = ptr::null_mut();

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
    log_msg("Prism: Initialize called");
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

extern "C" {
    fn mach_absolute_time() -> u64;
    fn CFStringCompare(theString1: CFStringRef, theString2: CFStringRef, compareOptions: CFOptionFlags) -> CFComparisonResult;
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
    
    let res = match object_id {
        id if id == kAudioObjectPlugInObject => {
            if selector == kAudioObjectPropertyBaseClass ||
               selector == kAudioObjectPropertyClass ||
               selector == kAudioObjectPropertyOwner ||
               selector == kAudioObjectPropertyManufacturer ||
               selector == kAudioObjectPropertyOwnedObjects ||
               selector == kAudioPlugInPropertyDeviceList ||
               selector == kAudioPlugInPropertyTranslateUIDToDevice ||
               selector == kAudioPlugInPropertyResourceBundle {
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
               selector == kAudioDevicePropertyStreams ||
               selector == kAudioDevicePropertyDeviceUID ||
               selector == kAudioDevicePropertyModelUID ||
               selector == kAudioDevicePropertyTransportType ||
               selector == kAudioDevicePropertyDeviceName ||
               selector == kAudioDevicePropertyDeviceIsRunning ||
               selector == kAudioDevicePropertyDeviceCanBeDefaultDevice ||
               selector == kAudioDevicePropertyDeviceCanBeDefaultSystemDevice ||
               selector == kAudioDevicePropertySafetyOffset ||
               selector == kAudioDevicePropertyLatency ||
               selector == kAudioDevicePropertyNominalSampleRate ||
               selector == kAudioDevicePropertyAvailableNominalSampleRates {
                true
            } else {
                false
            }
        },
        INPUT_STREAM_ID | OUTPUT_STREAM_ID => {
            if selector == kAudioObjectPropertyBaseClass ||
               selector == kAudioObjectPropertyClass ||
               selector == kAudioObjectPropertyOwner ||
               selector == kAudioStreamPropertyDirection ||
               selector == kAudioStreamPropertyTerminalType ||
               selector == kAudioStreamPropertyStartingChannel ||
               selector == kAudioStreamPropertyVirtualFormat ||
               selector == kAudioStreamPropertyPhysicalFormat ||
               selector == kAudioStreamPropertyPhysicalFormats ||
               selector == kAudioStreamPropertyAvailableVirtualFormats ||
               selector == kAudioStreamPropertyAvailablePhysicalFormats {
                true
            } else {
                false
            }
        },
        _ => false
    };

    log_msg(&format!("Prism: HasProperty called. Object: {}, Selector: {} -> {}", object_id, selector, res));
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
    log_msg(&format!("Prism: GetPropertyDataSize called. Object: {}, Selector: {}", object_id, selector));

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
            } else {
                return kAudioHardwareUnknownPropertyError as OSStatus;
            }
        },
        DEVICE_ID => {
            if selector == kAudioObjectPropertyBaseClass ||
               selector == kAudioObjectPropertyClass ||
               selector == kAudioObjectPropertyOwner ||
               selector == kAudioDevicePropertyTransportType ||
               selector == kAudioDevicePropertyDeviceIsRunning ||
               selector == kAudioDevicePropertyDeviceCanBeDefaultDevice ||
               selector == kAudioDevicePropertyDeviceCanBeDefaultSystemDevice ||
               selector == kAudioDevicePropertySafetyOffset ||
               selector == kAudioDevicePropertyLatency {
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioObjectPropertyManufacturer ||
                      selector == kAudioDevicePropertyDeviceUID ||
                      selector == kAudioDevicePropertyModelUID ||
                      selector == kAudioDevicePropertyDeviceName {
                *_out_data_size = std::mem::size_of::<CFStringRef>() as UInt32;
            } else if selector == kAudioObjectPropertyOwnedObjects || selector == kAudioDevicePropertyStreams {
                *_out_data_size = (2 * std::mem::size_of::<AudioObjectID>()) as UInt32;
            } else if selector == kAudioDevicePropertyNominalSampleRate {
                *_out_data_size = std::mem::size_of::<Float64>() as UInt32;
            } else if selector == kAudioDevicePropertyAvailableNominalSampleRates {
                *_out_data_size = std::mem::size_of::<AudioValueRange>() as UInt32;
            } else {
                return kAudioHardwareUnknownPropertyError as OSStatus;
            }
        },
        INPUT_STREAM_ID | OUTPUT_STREAM_ID => {
            if selector == kAudioObjectPropertyBaseClass ||
               selector == kAudioObjectPropertyClass ||
               selector == kAudioObjectPropertyOwner ||
               selector == kAudioStreamPropertyDirection ||
               selector == kAudioStreamPropertyTerminalType ||
               selector == kAudioStreamPropertyStartingChannel {
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
    let address = *_address;
    let selector = address.mSelector;
    log_msg(&format!("Prism: GetPropertyData called. Object: {}, Selector: {}", object_id, selector));

    match object_id {
        id if id == kAudioObjectPlugInObject => {
            if selector == kAudioObjectPropertyBaseClass {
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
                return kAudioHardwareUnknownPropertyError as OSStatus;
            }
        },
        DEVICE_ID => {
            if selector == kAudioObjectPropertyBaseClass {
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
            } else if selector == kAudioDevicePropertyDeviceName {
                let out = _out_data as *mut CFStringRef;
                *out = CFStringCreateWithCString(ptr::null(), "Prism\0".as_ptr() as *const i8, kCFStringEncodingUTF8);
                *_out_data_size = std::mem::size_of::<CFStringRef>() as UInt32;
            } else if selector == kAudioDevicePropertyTransportType {
                let out = _out_data as *mut UInt32;
                *out = kAudioDeviceTransportTypeVirtual;
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioDevicePropertyDeviceIsRunning {
                let out = _out_data as *mut UInt32;
                *out = 0;
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioDevicePropertyDeviceCanBeDefaultDevice {
                let out = _out_data as *mut UInt32;
                *out = 1;
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioDevicePropertyDeviceCanBeDefaultSystemDevice {
                let out = _out_data as *mut UInt32;
                *out = 1;
                *_out_data_size = std::mem::size_of::<UInt32>() as UInt32;
            } else if selector == kAudioDevicePropertySafetyOffset || selector == kAudioDevicePropertyLatency {
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
            } else if selector == kAudioObjectPropertyOwnedObjects || selector == kAudioDevicePropertyStreams {
                let out = _out_data as *mut AudioObjectID;
                *out.offset(0) = INPUT_STREAM_ID;
                *out.offset(1) = OUTPUT_STREAM_ID;
                *_out_data_size = (2 * std::mem::size_of::<AudioObjectID>()) as UInt32;
            } else {
                return kAudioHardwareUnknownPropertyError as OSStatus;
            }
        },
        INPUT_STREAM_ID | OUTPUT_STREAM_ID => {
            if selector == kAudioObjectPropertyBaseClass {
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

unsafe extern "C" fn start_io(
    _self: AudioServerPlugInDriverRef,
    _device_id: AudioObjectID,
    _client_id: UInt32,
) -> OSStatus {
    log_msg("Prism: StartIO called");
    0
}

unsafe extern "C" fn stop_io(
    _self: AudioServerPlugInDriverRef,
    _device_id: AudioObjectID,
    _client_id: UInt32,
) -> OSStatus {
    log_msg("Prism: StopIO called");
    0
}

unsafe extern "C" fn get_zero_timestamp(
    _self: AudioServerPlugInDriverRef,
    _device_id: AudioObjectID,
    _client_id: UInt32,
    out_sample_time: *mut Float64,
    out_host_time: *mut UInt64,
    out_seed: *mut UInt64,
) -> OSStatus {
    // Do not log here to avoid flooding
    // log_msg("Prism: GetZeroTimeStamp called");
    
    let now = mach_absolute_time();
    // A real driver would calculate sample time based on a start time and sample rate.
    // For now, we just return a dummy incrementing time or just 0 if we are not "running".
    // But since we return 0 for StartIO, the system thinks we are running.
    
    // We should probably track if we are running.
    // But for now, let's just return valid pointers.
    
    *out_sample_time = 0.0; // TODO: Implement proper timing
    *out_host_time = now;
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
fn log_msg(msg: &str) {
    unsafe {
        let c_msg = std::ffi::CString::new(msg).unwrap();
        libc::syslog(libc::LOG_NOTICE, c_msg.as_ptr());
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
            let driver = Box::new(PrismDriver {
                _vtable: &raw const DRIVER_VTABLE,
                ref_count: AtomicU32::new(1),
                host: None,
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
