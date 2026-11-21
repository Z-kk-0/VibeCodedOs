use super::memory::{AllocateType, MemoryDescriptor, MemoryMapInfo, MemoryType};
use super::{Guid, Handle, Status, TableHeader};
use core::ffi::c_void;

/// Placeholder for functions we don't call; keeps the layout right.
type Unused = *mut c_void;

/// Firmware services available until `ExitBootServices`. UEFI spec 2.10, §4.4.
///
/// Only the entries up to `locate_protocol` are listed. We only ever look
/// at this table through the firmware's pointer, so the missing tail is fine.
#[repr(C)]
pub struct BootServices {
    header: TableHeader,
    raise_tpl: Unused,
    restore_tpl: Unused,
    allocate_pages: unsafe extern "efiapi" fn(
        allocate_type: AllocateType,
        memory_type: MemoryType,
        pages: usize,
        address: *mut u64,
    ) -> Status,
    free_pages: Unused,
    get_memory_map: unsafe extern "efiapi" fn(
        map_size: *mut usize,
        map: *mut MemoryDescriptor,
        map_key: *mut usize,
        descriptor_size: *mut usize,
        descriptor_version: *mut u32,
    ) -> Status,
    allocate_pool: Unused,
    free_pool: Unused,
    // Event and timer services.
    create_event: Unused,
    set_timer: Unused,
    wait_for_event: Unused,
    signal_event: Unused,
    close_event: Unused,
    check_event: Unused,
    // Protocol handler services.
    install_protocol_interface: Unused,
    reinstall_protocol_interface: Unused,
    uninstall_protocol_interface: Unused,
    handle_protocol: Unused,
    reserved: Unused,
    register_protocol_notify: Unused,
    locate_handle: Unused,
    locate_device_path: Unused,
    install_configuration_table: Unused,
    // Image services.
    load_image: Unused,
    start_image: Unused,
    exit: Unused,
    unload_image: Unused,
    exit_boot_services: unsafe extern "efiapi" fn(image: Handle, map_key: usize) -> Status,
    // Miscellaneous services.
    get_next_monotonic_count: Unused,
    stall: Unused,
    set_watchdog_timer: Unused,
    // Driver support services.
    connect_controller: Unused,
    disconnect_controller: Unused,
    // Open and close protocol services.
    open_protocol: Unused,
    close_protocol: Unused,
    open_protocol_information: Unused,
    // Library services.
    protocols_per_handle: Unused,
    locate_handle_buffer: Unused,
    locate_protocol: unsafe extern "efiapi" fn(
        protocol: *const Guid,
        registration: *mut c_void,
        interface: *mut *mut c_void,
    ) -> Status,
}

impl BootServices {
    /// Allocates `count` 4 KiB pages anywhere, tagged with `memory_type`.
    /// Returns the physical address (UEFI identity-maps, so it's also a pointer).
    pub fn allocate_pages(&self, memory_type: MemoryType, count: usize) -> Result<u64, Status> {
        let mut address = 0;
        // SAFETY: `address` is a valid out-pointer for the duration of the call.
        let status = unsafe {
            (self.allocate_pages)(AllocateType::ANY_PAGES, memory_type, count, &mut address)
        };
        status.into_result().map(|()| address)
    }

    /// Like `allocate_pages`, but the pages must end at or below `limit`.
    pub fn allocate_pages_below(
        &self,
        memory_type: MemoryType,
        count: usize,
        limit: u64,
    ) -> Result<u64, Status> {
        // In: the highest allowed address of the last byte. Out: the start.
        let mut address = limit - 1;
        // SAFETY: `address` is a valid in/out pointer for the duration of the call.
        let status = unsafe {
            (self.allocate_pages)(AllocateType::MAX_ADDRESS, memory_type, count, &mut address)
        };
        status.into_result().map(|()| address)
    }

    /// Like `allocate_pages`, but zero-filled and as a byte slice. Rounds
    /// up to whole pages. Never freed – the kernel reclaims it later.
    pub fn allocate_zeroed(
        &self,
        memory_type: MemoryType,
        bytes: usize,
    ) -> Result<&'static mut [u8], Status> {
        const PAGE_SIZE: usize = 4096;
        let pages = bytes.div_ceil(PAGE_SIZE);
        let address = self.allocate_pages(memory_type, pages)?;
        // SAFETY: fresh, identity-mapped pages that only we own; zeroed before
        // the slice is created so every byte is initialized.
        unsafe {
            let start = address as *mut u8;
            core::ptr::write_bytes(start, 0, pages * PAGE_SIZE);
            Ok(core::slice::from_raw_parts_mut(start, pages * PAGE_SIZE))
        }
    }

    /// How big the memory map is right now (it grows with every allocation).
    pub fn memory_map_size(&self) -> Result<MemoryMapInfo, Status> {
        match self.read_memory_map(&mut []) {
            (Status::BUFFER_TOO_SMALL, info) => Ok(info),
            (status, _) => Err(status),
        }
    }

    /// Writes the current memory map into `buffer`.
    pub fn memory_map(&self, buffer: &mut [u8]) -> Result<MemoryMapInfo, Status> {
        let (status, info) = self.read_memory_map(buffer);
        status.into_result().map(|()| info)
    }

    fn read_memory_map(&self, buffer: &mut [u8]) -> (Status, MemoryMapInfo) {
        let mut info = MemoryMapInfo {
            size: buffer.len(),
            key: 0,
            descriptor_size: 0,
        };
        let mut descriptor_version = 0;
        // SAFETY: all out-pointers are valid; `info.size` tells the firmware
        // how much room `buffer` has, so it never writes past its end.
        let status = unsafe {
            (self.get_memory_map)(
                &mut info.size,
                buffer.as_mut_ptr().cast(),
                &mut info.key,
                &mut info.descriptor_size,
                &mut descriptor_version,
            )
        };
        (status, info)
    }

    /// The first instance of the protocol `guid` the firmware knows, if any
    /// (spec §7.3.16). Valid until `ExitBootServices`.
    pub fn locate_protocol(&self, guid: Guid) -> Option<*mut c_void> {
        let mut interface = core::ptr::null_mut();
        // SAFETY: `guid` and `interface` are valid for the duration of the
        // call; no registration key is passed (null is allowed).
        let status =
            unsafe { (self.locate_protocol)(&guid, core::ptr::null_mut(), &mut interface) };
        (status.into_result().is_ok() && !interface.is_null()).then_some(interface)
    }

    /// Ends the firmware's reign. Fails if the memory map changed since
    /// `map_key` was read.
    ///
    /// # Safety
    /// On success, nothing may call into boot services or any protocol
    /// (e.g. the console) ever again.
    pub unsafe fn exit_boot_services(&self, image: Handle, map_key: usize) -> Result<(), Status> {
        // SAFETY: forwarded to the caller.
        unsafe { (self.exit_boot_services)(image, map_key) }.into_result()
    }
}
