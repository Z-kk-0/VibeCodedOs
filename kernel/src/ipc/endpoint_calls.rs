//! Making endpoints and their sending ends at run time, and revoking
//! sending ends per badge (DECISIONS.md, 021).

use super::endpoint::Endpoint;
use super::message::{write_error, write_handle};
use super::object::{Capability, EndpointCapability};
use super::transfer::process;
use crate::arch::InterruptFrame;
use crate::task::Tasks;
use capability::Handle;
use scheduling::CoreId;

/// `endpoint_new()` – returns the receiving end.
pub fn endpoint_new(tasks: &mut Tasks, core: CoreId, frame: &mut InterruptFrame) {
    let me = tasks.current_id(core);
    match process(tasks, me) {
        Ok(process) => {
            let handle = process
                .handles
                .insert(Capability::receiving_end(&Endpoint::new()));
            write_handle(frame, handle);
        }
        Err(_) => write_error(frame, abi::ERROR_WRONG_KIND),
    }
}

/// `mint(receiving_end, badge)` – returns a new sending end.
pub fn mint(tasks: &mut Tasks, core: CoreId, frame: &mut InterruptFrame) {
    match receiving_end(tasks, core, frame.rdi) {
        Ok(end) => {
            let sending = Capability::Endpoint(end.mint(frame.rsi));
            let me = tasks.current_id(core);
            let handle = process(tasks, me)
                .expect("checked by receiving_end")
                .handles
                .insert(sending);
            write_handle(frame, handle);
        }
        Err(error) => write_error(frame, error),
    }
}

/// `revoke(receiving_end, badge)`.
pub fn revoke(tasks: &mut Tasks, core: CoreId, frame: &mut InterruptFrame) {
    match receiving_end(tasks, core, frame.rdi) {
        Ok(end) => {
            end.endpoint.revoke(frame.rsi);
            write_error(frame, 0);
        }
        Err(error) => write_error(frame, error),
    }
}

/// Only who can receive on an endpoint may mint and revoke for it.
fn receiving_end(tasks: &mut Tasks, core: CoreId, handle: u64) -> Result<EndpointCapability, u64> {
    let me = tasks.current_id(core);
    let process = process(tasks, me).map_err(|_| abi::ERROR_WRONG_KIND)?;
    match process.handles.get(Handle(handle)) {
        Some(Capability::Endpoint(end)) if end.can_receive => Ok(end.clone()),
        Some(_) => Err(abi::ERROR_WRONG_KIND),
        None => Err(abi::ERROR_BAD_HANDLE),
    }
}
