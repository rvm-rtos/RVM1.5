use core::slice;

use super::{apic, cpu};
use crate::error::HvResult;
use crate::memory::{addr::phys_to_virt, PhysAddr, PAGE_SIZE};
use crate::percpu::PerCpu;

const START_PAGE_IDX: u8 = 6;
const START_PAGE_COUNT: usize = 1;
const START_PAGE_PADDR: usize = START_PAGE_IDX as usize * PAGE_SIZE;

core::arch::global_asm!(
    include_str!("boot_rt.S"),
    start_page_paddr = const START_PAGE_PADDR,
);

#[allow(clippy::uninit_assumed_init)]
pub unsafe fn start_rt_cpus(entry_paddr: PhysAddr) -> HvResult {
    extern "C" {
        fn ap_start();
        fn ap_end();
    }
    const U64_PER_PAGE: usize = PAGE_SIZE / 8;

    let start_page_ptr = phys_to_virt(START_PAGE_PADDR) as *mut u64;
    let start_page = slice::from_raw_parts_mut(start_page_ptr, U64_PER_PAGE * START_PAGE_COUNT);
    let mut backup: [u64; U64_PER_PAGE * START_PAGE_COUNT] =
        core::mem::MaybeUninit::uninit().assume_init();
    backup.copy_from_slice(start_page);
    core::ptr::copy_nonoverlapping(
        ap_start as *const u64,
        start_page_ptr,
        (ap_end as usize - ap_start as usize) / 8,
    );
    start_page[U64_PER_PAGE - 1] = entry_paddr as _; // entry

    let max_cpus = crate::header::HvHeader::get().max_cpus;
    let mut new_cpu_id = PerCpu::entered_cpus();
    for apic_id in 0..max_cpus {
        if apic::apic_to_cpu_id(apic_id) == u32::MAX {
            if new_cpu_id >= max_cpus {
                break;
            }
            let current_entered_cpus = PerCpu::entered_cpus();
            let stack_top = PerCpu::from_id_mut(new_cpu_id).stack_top();
            start_page[U64_PER_PAGE - 3] = stack_top as u64; // stack
            apic::start_ap(apic_id, START_PAGE_IDX);
            new_cpu_id += 1;

            // wait for max 100ms
            let cycle_end = cpu::current_cycle() + 100 * 1000 * cpu::frequency() as u64;
            while PerCpu::entered_cpus() <= current_entered_cpus && cpu::current_cycle() < cycle_end
            {
                core::hint::spin_loop();
            }
        }
    }
    start_page.copy_from_slice(&backup);
    Ok(())
}

pub unsafe fn shutdown_rt_cpus() -> HvResult {
    let header = crate::header::HvHeader::get();
    for apic_id in header.vm_cpus()..header.max_cpus {
        apic::shutdown_ap(apic_id);
    }
    Ok(())
}
