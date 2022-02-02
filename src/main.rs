#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#![cfg_attr(test, allow(dead_code))]
#![feature(asm_sym)]
#![feature(asm_const)]
#![feature(lang_items)]
#![feature(concat_idents)]
#![feature(naked_functions)]
#![allow(unaligned_references)]

#[macro_use]
extern crate alloc;
#[macro_use]
extern crate log;
#[macro_use]
extern crate lazy_static;

#[macro_use]
mod logging;
#[macro_use]
mod error;

mod cell;
mod config;
mod consts;
mod header;
mod hypercall;
mod memory;
mod percpu;
mod stats;

#[cfg(not(test))]
mod lang;

#[cfg(target_arch = "x86_64")]
#[path = "arch/x86_64/mod.rs"]
mod arch;

use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use config::HvSystemConfig;
use error::HvResult;
use header::HvHeader;
use percpu::{entered_cpus, PerCpu};

static INITED_CPUS: AtomicU32 = AtomicU32::new(0);
static INIT_EARLY_OK: AtomicU32 = AtomicU32::new(0);
static INIT_LATE_OK: AtomicU32 = AtomicU32::new(0);
static ERROR_NUM: AtomicI32 = AtomicI32::new(0);

fn has_err() -> bool {
    ERROR_NUM.load(Ordering::Acquire) != 0
}

fn wait_for(condition: impl Fn() -> bool) -> HvResult {
    while !has_err() && condition() {
        core::hint::spin_loop();
    }
    if has_err() {
        hv_result_err!(EBUSY, "Other cpu init failed!")
    } else {
        Ok(())
    }
}

fn wait_for_counter(counter: &AtomicU32, max_value: u32) -> HvResult {
    wait_for(|| counter.load(Ordering::Acquire) < max_value)
}

fn primary_init_early() -> HvResult {
    logging::init();
    info!("Primary CPU init early...");

    let system_config = HvSystemConfig::get();
    println!(
        "\n\
        Initializing hypervisor...\n\
        config_signature = {:?}\n\
        config_revision = {}\n\
        build_mode = {}\n\
        log_level = {}\n\
        arch = {}\n\
        vendor = {}\n\
        stats = {}\n\
        ",
        core::str::from_utf8(&system_config.signature),
        system_config.revision,
        option_env!("MODE").unwrap_or(""),
        option_env!("LOG").unwrap_or(""),
        option_env!("ARCH").unwrap_or(""),
        option_env!("VENDOR").unwrap_or(""),
        option_env!("STATS").unwrap_or("off"),
    );

    memory::init_heap();
    system_config.check()?;
    info!("Hypervisor header: {:#x?}", HvHeader::get());
    debug!("System config: {:#x?}", system_config);

    memory::init_frame_allocator();
    memory::init_hv_page_table()?;
    cell::init()?;
    arch::init_early()?;

    INIT_EARLY_OK.store(1, Ordering::Release);
    Ok(())
}

fn primary_init_late() -> HvResult {
    info!("Primary CPU init late...");

    unsafe { arch::cpu::start_rt_cpus()? };

    INIT_LATE_OK.store(1, Ordering::Release);
    Ok(())
}

fn vm_main(cpu_data: &mut PerCpu, linux_sp: usize) -> HvResult {
    let cpu_id = cpu_data.id();
    let is_primary = cpu_id == 0;
    let vm_cpus = HvHeader::get().vm_cpus();
    wait_for(|| entered_cpus() < vm_cpus)?;
    println!(
        "{} CPU {} entered.",
        if is_primary { "Primary" } else { "Secondary" },
        cpu_id
    );

    if is_primary {
        primary_init_early()?;
    } else {
        wait_for_counter(&INIT_EARLY_OK, 1)?;
    }

    let inner = cpu_data.init_vm_cpu(linux_sp, cell::root_cell())?;
    println!("CPU {} init OK.", cpu_id);
    INITED_CPUS.fetch_add(1, Ordering::SeqCst);
    wait_for_counter(&INITED_CPUS, vm_cpus)?;

    if is_primary {
        primary_init_late()?;
    } else {
        wait_for_counter(&INIT_LATE_OK, 1)?;
    }

    inner.activate_vmm()
}

fn rt_main(cpu_data: &mut PerCpu) -> ! {
    println!("RT CPU {} entered.", cpu_data.id());
    cpu_data.init_rt_cpu().unwrap();
    loop {
        core::hint::spin_loop();
    }
}

extern "sysv64" fn vm_cpu_entry(cpu_data: &mut PerCpu, linux_sp: usize) -> i32 {
    if let Err(e) = vm_main(cpu_data, linux_sp) {
        error!("{:?}", e);
        ERROR_NUM.store(e.code(), Ordering::Release);
    }
    let code = ERROR_NUM.load(Ordering::Acquire);
    println!(
        "CPU {} return back to driver with code {}.",
        cpu_data.id(),
        code
    );
    code
}

extern "sysv64" fn rt_cpu_entry() -> ! {
    let cpu_data = PerCpu::new().expect("Failed to allocate RT CPU");
    rt_main(cpu_data)
}
