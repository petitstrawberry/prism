mod driver;

use coreaudio_sys::*;

#[no_mangle]
pub extern "C" fn AudioServerPlugInMain(
    _allocator: CFAllocatorRef,
    _requested_type_uuid: CFUUIDRef,
) -> *mut libc::c_void {
    // unsafe {
    // let ident = std::ffi::CString::new("PrismDriver").unwrap();
    // libc::openlog(ident.as_ptr(), libc::LOG_CONS | libc::LOG_PID, libc::LOG_USER);

    // let msg = std::ffi::CString::new("Prism initialized (v2)").unwrap();
    // libc::syslog(libc::LOG_NOTICE, msg.as_ptr());

    // Return our driver interface
    driver::create_driver() as *mut libc::c_void
    // }
}
