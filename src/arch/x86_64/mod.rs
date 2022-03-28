#[macro_use]
mod context;
mod apic;
mod boot_rt;
mod cpuid;
mod entry;
mod exception;
mod page_table;
mod percpu;
mod segmentation;
mod tables;

pub mod cpu;
pub mod serial;
pub mod vmm;

pub use boot_rt::{shutdown_rt_cpus, start_rt_cpus};
pub use context::{GeneralRegisters, LinuxContext};
pub use exception::ExceptionType;
pub use page_table::PageTable as HostPageTable;
pub use page_table::PageTable as GuestPageTable;
pub use page_table::PageTableImmut as GuestPageTableImmut;
pub use percpu::ArchPerCpu;
pub use vmm::NestedPageTable;

pub fn init_early() -> crate::error::HvResult {
    apic::init()
}
