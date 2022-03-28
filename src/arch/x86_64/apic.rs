use libvmm::msr::Msr;
use spin::{Once, RwLock};
use x86::apic::{x2apic::X2APIC, xapic::XAPIC, ApicControl, ApicId};

use alloc::sync::Arc;

use crate::error::HvResult;
use crate::memory::addr::{phys_to_virt, PhysAddr};
use crate::memory::{hv_page_table, MemFlags, MemoryRegion, PAGE_SIZE};

const APIC_BASE: PhysAddr = 0xFEE0_0000;
const MAX_APIC_ID: u32 = 254;

bitflags::bitflags! {
    /// IA32_APIC_BASE MSR.
    struct ApicBase: u64 {
        /// Processor is BSP.
        const BSP   = 1 << 8;
        /// Enable x2APIC mode.
        const EXTD  = 1 << 10;
        /// xAPIC global enable/disable.
        const EN    = 1 << 11;
    }
}

impl ApicBase {
    pub fn read() -> Self {
        unsafe { Self::from_bits_unchecked(Msr::IA32_APIC_BASE.read()) }
    }
}

pub(super) struct LocalApic {
    inner: Arc<RwLock<dyn ApicControl>>,
    is_x2apic: bool,
}

unsafe impl Send for LocalApic {}
unsafe impl Sync for LocalApic {}

impl LocalApic {
    pub fn new() -> HvResult<Self> {
        let base = ApicBase::read();
        if base.contains(ApicBase::EXTD) {
            info!("Using x2APIC.");
            Ok(Self {
                inner: Arc::new(RwLock::new(X2APIC::new())),
                is_x2apic: true,
            })
        } else if base.contains(ApicBase::EN) {
            info!("Using xAPIC.");
            let base_vaddr = phys_to_virt(APIC_BASE);
            let mut hv_pt = hv_page_table().write();
            hv_pt.insert(MemoryRegion::new_with_offset_mapper(
                phys_to_virt(APIC_BASE),
                APIC_BASE,
                PAGE_SIZE,
                MemFlags::READ | MemFlags::WRITE | MemFlags::IO,
            ))?;
            let apic_region =
                unsafe { core::slice::from_raw_parts_mut(base_vaddr as _, PAGE_SIZE / 4) };
            Ok(Self {
                inner: Arc::new(RwLock::new(XAPIC::new(apic_region))),
                is_x2apic: false,
            })
        } else {
            hv_result_err!(EIO)
        }
    }

    pub fn id(&self) -> u32 {
        if self.is_x2apic {
            self.inner.read().id()
        } else {
            self.inner.read().id() >> 24
        }
    }
}

static LOCAL_APIC: Once<LocalApic> = Once::new();
static mut APIC_TO_CPU_ID: [u32; MAX_APIC_ID as usize + 1] = [u32::MAX; MAX_APIC_ID as usize + 1];

pub(super) fn lapic<'a>() -> &'a LocalApic {
    LOCAL_APIC.get().expect("Uninitialized Local APIC!")
}

pub(super) fn apic_to_cpu_id(apic_id: u32) -> u32 {
    if apic_id <= MAX_APIC_ID {
        unsafe { APIC_TO_CPU_ID[apic_id as usize] }
    } else {
        u32::MAX
    }
}

pub(super) fn init() -> HvResult {
    let lapic = LocalApic::new()?;
    LOCAL_APIC.call_once(|| lapic);
    Ok(())
}

pub(super) fn init_percpu(cpu_id: u32) -> HvResult {
    let apic_id = lapic().id();
    if apic_id > MAX_APIC_ID {
        return hv_result_err!(ERANGE);
    }
    unsafe { APIC_TO_CPU_ID[apic_id as usize] = cpu_id };
    Ok(())
}

pub(super) unsafe fn start_ap(apic_id: u32, start_page_idx: u8) {
    info!("Starting RT cpu {}...", apic_id);
    let apic_id = if lapic().is_x2apic {
        ApicId::X2Apic(apic_id)
    } else {
        ApicId::XApic(apic_id as u8)
    };

    // INIT-SIPI-SIPI Sequence
    let mut lapic = lapic().inner.write();
    lapic.ipi_init(apic_id);
    delay_us(10 * 1000); // 10ms
    lapic.ipi_startup(apic_id, start_page_idx);
    delay_us(200); // 200 us
    lapic.ipi_startup(apic_id, start_page_idx);
}

pub(super) unsafe fn shutdown_ap(apic_id: u32) {
    info!("Shutting down RT cpu {}...", apic_id);
    let apic_id = if lapic().is_x2apic {
        ApicId::X2Apic(apic_id)
    } else {
        ApicId::XApic(apic_id as u8)
    };

    lapic().inner.write().ipi_init(apic_id);
}

/// Spinning delay for specified amount of time on microseconds.
fn delay_us(us: u64) {
    let cycle_end = super::cpu::current_cycle() + us * super::cpu::frequency() as u64;
    while super::cpu::current_cycle() < cycle_end {
        core::hint::spin_loop();
    }
}
