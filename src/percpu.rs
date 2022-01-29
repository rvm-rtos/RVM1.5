use core::fmt::{Debug, Formatter, Result};
use core::sync::atomic::{AtomicU32, Ordering};

use crate::arch::vmm::{Vcpu, VcpuAccessGuestState};
use crate::arch::{cpu, ArchPerCpu, LinuxContext};
use crate::cell::Cell;
use crate::consts::{PER_CPU_ARRAY_PTR, PER_CPU_SIZE};
use crate::error::HvResult;
use crate::header::HvHeader;
use crate::memory::VirtAddr;

static ENTERED_CPUS: AtomicU32 = AtomicU32::new(0);
static ACTIVATED_CPUS: AtomicU32 = AtomicU32::new(0);

#[derive(Debug)]
pub struct RtPerCpuData;

pub struct VmPerCpuData {
    linux: LinuxContext,
    pub vcpu: Vcpu,
}

#[derive(Debug)]
enum PerCpuData {
    Uninit,
    Rt(RtPerCpuData),
    Vm(VmPerCpuData),
}

#[repr(C, align(4096))]
pub struct PerCpu {
    /// Referenced by arch::cpu::thread_pointer() for x86_64.
    self_vaddr: VirtAddr,
    arch: ArchPerCpu,
    id: u32,
    inner: PerCpuData,
    // Stack will be placed here.
}

impl PerCpu {
    pub fn new<'a>() -> HvResult<&'a mut Self> {
        if entered_cpus() >= HvHeader::get().max_cpus {
            return hv_result_err!(EINVAL);
        }

        let cpu_id = ENTERED_CPUS.fetch_add(1, Ordering::SeqCst);
        let ret = unsafe { Self::from_id_mut(cpu_id) };
        let vaddr = ret as *const _ as VirtAddr;
        ret.id = cpu_id;
        ret.self_vaddr = vaddr;
        unsafe { core::ptr::write(&mut ret.inner, PerCpuData::Uninit) };
        cpu::set_thread_pointer(vaddr);
        Ok(ret)
    }

    pub unsafe fn from_id_mut<'a>(cpu_id: u32) -> &'a mut Self {
        let vaddr = PER_CPU_ARRAY_PTR as VirtAddr + cpu_id as usize * PER_CPU_SIZE;
        &mut *(vaddr as *mut Self)
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn stack_top(&self) -> VirtAddr {
        self as *const _ as VirtAddr + PER_CPU_SIZE - 8
    }

    pub fn init_vm_cpu(&mut self, linux_sp: usize, cell: &Cell) -> HvResult<&mut VmPerCpuData> {
        info!("VM CPU {} init...", self.id);

        // Save CPU state used for linux.
        let linux = LinuxContext::load_from(linux_sp);

        // Activate hypervisor page table on each cpu.
        unsafe { crate::memory::hv_page_table().read().activate() };

        self.arch.init(self.id)?;
        self.inner = PerCpuData::Vm(VmPerCpuData {
            vcpu: Vcpu::new(&linux, cell)?,
            linux,
        });

        if let PerCpuData::Vm(inner) = &mut self.inner {
            Ok(inner)
        } else {
            hv_result_err!(EINVAL, "Failed to init VM CPU!")
        }
    }

    pub fn init_rt_cpu(&mut self) -> HvResult<&mut RtPerCpuData> {
        info!("RT CPU {} init...", self.id);

        // Activate hypervisor page table on each cpu.
        unsafe { crate::memory::hv_page_table().read().activate() };

        self.arch.init(self.id)?;
        self.inner = PerCpuData::Rt(RtPerCpuData);

        if let PerCpuData::Rt(inner) = &mut self.inner {
            Ok(inner)
        } else {
            hv_result_err!(EINVAL, "Failed to init RT CPU!")
        }
    }

    pub fn vm_cpu(&mut self) -> Option<&mut VmPerCpuData> {
        if let PerCpuData::Vm(inner) = &mut self.inner {
            Some(inner)
        } else {
            None
        }
    }

    pub fn rt_cpu(&mut self) -> Option<&mut RtPerCpuData> {
        if let PerCpuData::Rt(inner) = &mut self.inner {
            Some(inner)
        } else {
            None
        }
    }
}

impl VmPerCpuData {
    pub fn activate_vmm(&mut self) -> HvResult {
        println!("Activating hypervisor on CPU {}...", current().id());
        ACTIVATED_CPUS.fetch_add(1, Ordering::SeqCst);

        self.vcpu.enter(&self.linux)?;
        unreachable!()
    }

    pub fn deactivate_vmm(&mut self, ret_code: usize) -> HvResult {
        println!("Deactivating hypervisor on CPU {}...", current().id());
        ACTIVATED_CPUS.fetch_sub(1, Ordering::SeqCst);

        self.vcpu.set_return_val(ret_code);
        self.vcpu.exit(&mut self.linux)?;
        self.linux.restore();
        self.linux.return_to_linux(self.vcpu.regs());
    }

    pub fn fault(&mut self) -> HvResult {
        warn!("VCPU fault: {:#x?}", self);
        self.vcpu.inject_fault()?;
        Ok(())
    }
}

impl Debug for PerCpu {
    fn fmt(&self, f: &mut Formatter) -> Result {
        f.debug_struct("PerCpu")
            .field("id", &self.id)
            .field("self_vaddr", &self.self_vaddr)
            .field("inner", &self.inner)
            .finish()
    }
}

impl Debug for VmPerCpuData {
    fn fmt(&self, f: &mut Formatter) -> Result {
        f.debug_struct("VmPerCpuData")
            .field("vcpu", &self.vcpu)
            .finish()
    }
}

pub fn entered_cpus() -> u32 {
    ENTERED_CPUS.load(Ordering::Acquire)
}

pub fn activated_vm_cpus() -> u32 {
    ACTIVATED_CPUS.load(Ordering::Acquire)
}

pub fn current<'a>() -> &'a mut PerCpu {
    unsafe { &mut *(cpu::thread_pointer() as *mut PerCpu) }
}

pub fn current_vm_cpu<'a>() -> Option<&'a mut VmPerCpuData> {
    current().vm_cpu()
}

#[allow(dead_code)]
pub fn current_rt_cpu<'a>() -> Option<&'a mut RtPerCpuData> {
    current().rt_cpu()
}
