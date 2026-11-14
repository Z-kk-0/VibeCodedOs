//! The memory system calls: making a memory object and giving a handle up.

use super::message::{Delivery, Received, write_error};
use super::object::{self, Capability, OwnedMemory};
use super::transfer::process;
use crate::arch::InterruptFrame;
use crate::task::Tasks;
use capability::Handle;
use scheduling::CoreId;

/// `memory_new(pages)`.
pub fn memory_new(tasks: &mut Tasks, core: CoreId, frame: &mut InterruptFrame) {
    let me = tasks.current_id(core);
    let pages = frame.rdi;
    let result = (|| {
        let process = process(tasks, me).map_err(|_| abi::ERROR_WRONG_KIND)?;
        if pages == 0 || pages > abi::MAX_MEMORY_PAGES {
            return Err(abi::ERROR_OUT_OF_MEMORY);
        }
        let memory = object::Memory::new(pages).ok_or(abi::ERROR_OUT_OF_MEMORY)?;
        let address = process
            .map_frames(&memory.frames(), true)
            .ok_or(abi::ERROR_OUT_OF_MEMORY)?;
        let handle = process.handles.insert(Capability::Memory(OwnedMemory {
            memory,
            address,
            lent: false,
        }));
        Ok(Received {
            handle: Some(handle),
            memory: Some((address, pages, abi::ACCESS_OWNED)),
        })
    })();
    match result {
        Ok(received) => Delivery {
            badge: 0,
            words: [0; 4],
            received,
        }
        .write_to(frame),
        Err(error) => write_error(frame, error),
    }
}

/// `drop(handle)`.
pub fn drop_handle(tasks: &mut Tasks, core: CoreId, frame: &mut InterruptFrame) {
    let me = tasks.current_id(core);
    let result = process(tasks, me)
        .map_err(|_| abi::ERROR_WRONG_KIND)
        .and_then(|process| {
            let handle = Handle(frame.rdi);
            match process.handles.get(handle) {
                None => return Err(abi::ERROR_BAD_HANDLE),
                Some(Capability::Memory(owned)) if owned.lent => return Err(abi::ERROR_BUSY),
                Some(_) => {}
            }
            match process.handles.remove(handle) {
                Some(Capability::Memory(owned)) => {
                    process.unmap_pages(owned.address, owned.memory.pages() as usize);
                }
                Some(Capability::Borrowed(borrowed)) => {
                    process.unmap_pages(borrowed.address, borrowed.frames.len());
                }
                _ => {}
            }
            Ok(())
        });
    write_error(frame, result.err().unwrap_or(0));
}
