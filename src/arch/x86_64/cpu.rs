use core::slice;

use libvmm::msr::Msr;

use super::apic;
use super::cpuid::CpuId;
use crate::error::HvResult;
use crate::memory::{addr::phys_to_virt, hv_page_table, MemFlags, MemoryRegion, PAGE_SIZE};
use crate::percpu::PerCpu;

pub fn frequency() -> u16 {
    static CPU_FREQUENCY: spin::Once<u16> = spin::Once::new();
    *CPU_FREQUENCY.call_once(|| {
        const DEFAULT: u16 = 4000;
        CpuId::new()
            .get_processor_frequency_info()
            .map(|info| info.processor_base_frequency())
            .unwrap_or(DEFAULT)
            .max(DEFAULT)
    })
}

pub fn current_cycle() -> u64 {
    let mut aux = 0;
    unsafe { core::arch::x86_64::__rdtscp(&mut aux) }
}

pub fn current_time_nanos() -> u64 {
    current_cycle() * 1000 / frequency() as u64
}

pub fn thread_pointer() -> usize {
    let ret;
    unsafe { core::arch::asm!("mov {0}, gs:0", out(reg) ret, options(nostack)) }; // PerCpu::self_vaddr
    ret
}

pub fn set_thread_pointer(tp: usize) {
    unsafe { Msr::IA32_GS_BASE.write(tp as u64) };
}

core::arch::global_asm!(include_str!("boot_ap.S"));

#[allow(clippy::uninit_assumed_init)]
pub unsafe fn start_rt_cpus() -> HvResult {
    extern "C" {
        fn ap_start();
        fn ap_end();
    }
    const START_PAGE_IDX: u8 = 6;
    const START_PAGE_PADDR: usize = START_PAGE_IDX as usize * PAGE_SIZE;
    const U64_PER_PAGE: usize = PAGE_SIZE / 8;

    let mut hv_pt = hv_page_table().write();
    hv_pt.insert(MemoryRegion::new_with_offset_mapper(
        START_PAGE_PADDR,
        START_PAGE_PADDR,
        PAGE_SIZE,
        MemFlags::READ | MemFlags::WRITE | MemFlags::EXECUTE,
    ))?;

    let start_page_ptr = phys_to_virt(START_PAGE_PADDR) as *mut u64;
    let start_page = slice::from_raw_parts_mut(start_page_ptr, U64_PER_PAGE * 3); // 3 pages
    let mut backup: [u64; U64_PER_PAGE * 3] = core::mem::MaybeUninit::uninit().assume_init();
    backup.copy_from_slice(start_page);
    core::ptr::copy_nonoverlapping(
        ap_start as *const u64,
        start_page_ptr,
        (ap_end as usize - ap_start as usize) / 8,
    );
    start_page[U64_PER_PAGE - 1] = x86::controlregs::cr3(); // cr3
    start_page[U64_PER_PAGE - 2] = crate::rt_cpu_entry as usize as _; // entry

    let max_cpus = crate::header::HvHeader::get().max_cpus;
    let mut new_cpu_id = PerCpu::entered_cpus();
    for apic_id in 0..max_cpus {
        if apic::apic_to_cpu_id(apic_id) == u32::MAX {
            if new_cpu_id >= max_cpus {
                break;
            }
            let entered_cpus = PerCpu::entered_cpus();
            let stack_top = PerCpu::from_id_mut(new_cpu_id).stack_top();
            start_page[U64_PER_PAGE - 3] = stack_top as u64; // stack
            apic::start_ap(apic_id, START_PAGE_IDX);
            new_cpu_id += 1;

            // wait for max 100ms
            let cycle_end = current_cycle() + 100 * 1000 * frequency() as u64;
            while PerCpu::entered_cpus() <= entered_cpus && current_cycle() < cycle_end {
                core::hint::spin_loop();
            }
        }
    }
    start_page.copy_from_slice(&backup);
    Ok(())
}
