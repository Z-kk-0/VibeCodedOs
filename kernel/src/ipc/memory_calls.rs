//! The memory system calls: making a memory object and giving a handle up.

use super::delivery::{fail_reply, release_receiving_end};
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
    let (pages, align) = (frame.rdi, frame.rsi.max(paging::PAGE_SIZE));
    let result = (|| {
        let process = process(tasks, me).map_err(|_| abi::ERROR_WRONG_KIND)?;
        if pages == 0 || pages > abi::MAX_MEMORY_PAGES {
            return Err(abi::ERROR_OUT_OF_MEMORY);
        }
        if !align.is_power_of_two() {
            return Err(abi::ERROR_BAD_ARGUMENT);
        }
        let memory = object::Memory::new(pages).ok_or(abi::ERROR_OUT_OF_MEMORY)?;
        let address = process
            .map_frames_aligned(&memory.frames(), true, memory.caching(), align)
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
            reply: None,
        }
        .write_to(frame),
        Err(error) => write_error(frame, error),
    }
}

/// `drop(handle)`. Giving up a reply handle tells its caller; giving up
/// the last receiving end closes the endpoint.
pub fn drop_handle(tasks: &mut Tasks, core: CoreId, frame: &mut InterruptFrame) {
    let me = tasks.current_id(core);
    let handle = Handle(frame.rdi);
    let removed = process(tasks, me)
        .map_err(|_| abi::ERROR_WRONG_KIND)
        .and_then(|process| {
            match process.handles.get(handle) {
                None => return Err(abi::ERROR_BAD_HANDLE),
                Some(Capability::Memory(owned)) if owned.lent => return Err(abi::ERROR_BUSY),
                Some(_) => {}
            }
            let removed = process.handles.remove(handle).expect("checked above");
            match &removed {
                Capability::Memory(owned) => {
                    process.release_pages(owned.address, owned.memory.pages() as usize);
                }
                Capability::Borrowed(borrowed) => {
                    process.release_pages(borrowed.address, borrowed.frames.len());
                }
                _ => {}
            }
            Ok(removed)
        });
    let status = removed_status(&removed);
    match removed {
        Ok(Capability::Reply(reply)) => fail_reply(tasks, reply, abi::ERROR_PEER_GONE),
        Ok(Capability::Endpoint(end)) if end.can_receive => release_receiving_end(tasks, end),
        Ok(Capability::Child(child)) => crate::task::forget_exit(child),
        _ => {}
    }
    write_error(frame, status);
}

fn removed_status<T>(removed: &Result<T, u64>) -> u64 {
    removed.as_ref().err().copied().unwrap_or(0)
}

/// `memory_info(handle)` – where memory we hold is mapped, and how much.
pub fn memory_info(tasks: &mut Tasks, core: CoreId, frame: &mut InterruptFrame) {
    let me = tasks.current_id(core);
    let info = process(tasks, me)
        .map_err(|_| abi::ERROR_WRONG_KIND)
        .and_then(|process| match process.handles.get(Handle(frame.rdi)) {
            Some(Capability::Memory(owned)) if owned.lent => Err(abi::ERROR_BUSY),
            Some(Capability::Memory(owned)) => {
                Ok((owned.address, owned.memory.pages(), abi::ACCESS_OWNED))
            }
            Some(Capability::Borrowed(borrowed)) => Ok((
                borrowed.address,
                borrowed.frames.len() as u64,
                borrowed.access,
            )),
            Some(_) => Err(abi::ERROR_WRONG_KIND),
            None => Err(abi::ERROR_BAD_HANDLE),
        });
    match info {
        Ok(memory) => Delivery {
            badge: 0,
            words: [0; 4],
            received: Received {
                handle: Some(Handle(frame.rdi)),
                memory: Some(memory),
            },
            reply: None,
        }
        .write_to(frame),
        Err(error) => write_error(frame, error),
    }
}
